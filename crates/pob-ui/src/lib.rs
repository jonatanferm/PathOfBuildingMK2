//! egui UI for Path of Building MK2. Phase 4a: passive tree screen with live stats.

use std::collections::HashSet;
use std::path::PathBuf;

use eframe::egui;
use pob_data::{NodeId, PassiveTree};
use pob_engine::{character::ClassRef, Character, Output, SkillRegistry};

mod calcs_tab;
mod color_codes;
mod config_tab;
mod import_export_tab;
mod items_tab;
mod notes_tab;
mod pathfind;
mod skills_tab;
mod tree_layout;
mod tree_renderer;
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
    /// Mod-DB the last compute pass produced. Used by the Calcs tab's stat
    /// breakdown panel to surface contributing mods. Lives outside `output`
    /// because Output is a flat key→f64 dictionary; the breakdown needs the
    /// underlying Mod list with sources / tags.
    last_env: Option<pob_engine::Env>,
    search: String,
    active_tab: Tab,
    items_state: items_tab::ItemsTabState,
    skills_state: skills_tab::SkillsTabState,
    calcs_state: calcs_tab::CalcsTabState,
    import_export_state: import_export_tab::ImportExportTabState,
    notes_state: notes_tab::NotesTabState,
    skills: SkillRegistry,
    bases: Option<pob_data::bases::ItemBaseSet>,
    /// Path of the currently-open build file, if any. Used by Save vs Save As.
    current_build_path: Option<std::path::PathBuf>,
    status_message: Option<(StatusKind, String)>,
    /// Available tree versions found in `data/trees/`.
    tree_versions: Vec<String>,
    /// Currently-loaded tree version.
    tree_version: String,
    /// Path to the data root resolved at startup.
    data_root: std::path::PathBuf,
    /// Hash of the inputs `compute_full` last ran with — skip recompute when unchanged.
    last_compute_hash: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StatusKind {
    Info,
    Error,
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
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Install the wgpu tree renderer once at app boot, uploading the two
        // skill atlases (active + inactive). With the wgpu backend forced in
        // `pob-desktop/main.rs`, `cc.wgpu_render_state` is always available;
        // the early-return path keeps tests / future backends from panicking.
        if let Some(rs) = cc.wgpu_render_state.as_ref() {
            if let Some(atlases) = load_atlases() {
                tree_renderer::TreeRenderer::install(rs, atlases);
            }
        }
        let state = match Self::load_initial() {
            Ok(loaded) => AppState::Loaded(loaded),
            Err(e) => AppState::Error(e),
        };
        Self { state }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn load_initial() -> Result<LoadedApp, String> {
        // Find data/ relative to the workspace root. pob-desktop runs from there.
        let candidates = [
            PathBuf::from("data"),
            PathBuf::from("../data"),
            PathBuf::from("../../data"),
        ];
        let data_root = candidates
            .iter()
            .find(|p| p.join("trees/index.json").is_file())
            .cloned()
            .ok_or_else(|| {
                "could not find data/trees/index.json — run `cargo run -p pob-extract --release` from the workspace root".to_owned()
            })?;
        // Resolve the available tree versions from the index so the UI can let the user
        // switch between 3_25 / 3_28 / etc. We pick the latest non-alternate, non-ruthless
        // version as the default (lexicographic last with a stable filter).
        let index_json = std::fs::read_to_string(data_root.join("trees/index.json"))
            .map_err(|e| format!("reading tree index: {e}"))?;
        let mut tree_versions: Vec<String> =
            pob_data::load_tree_index(&index_json).map_err(|e| format!("parsing tree index: {e}"))?;
        tree_versions.sort();
        let default_version = tree_versions
            .iter()
            .filter(|v| !v.contains("alternate") && !v.contains("ruthless"))
            .next_back()
            .cloned()
            .or_else(|| tree_versions.last().cloned())
            .ok_or_else(|| "no tree versions found".to_owned())?;
        let tree_path = data_root.join("trees").join(format!("{default_version}.json"));
        let tree_json =
            std::fs::read_to_string(&tree_path).map_err(|e| format!("reading tree: {e}"))?;
        let tree = pob_data::load_passive_tree(&tree_json)
            .map_err(|e| format!("parsing tree: {e}"))?;

        let bases = std::fs::read_to_string(data_root.join("bases.json"))
            .ok()
            .and_then(|j| pob_data::load_bases(&j).ok());

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

        let sprites = load_sprite_metadata();
        let tree_view = TreeView::new(&tree, sprites.as_ref());
        let character = Character::new(ClassRef::marauder(), 1);
        let (output, env) = pob_engine::compute_full_with_env(
            &character,
            &tree,
            Some(&skills),
            bases.as_ref(),
        );

        Ok(LoadedApp {
            tree,
            tree_view,
            character,
            output,
            last_env: Some(env),
            search: String::new(),
            active_tab: Tab::Tree,
            items_state: items_tab::ItemsTabState::default(),
            skills_state: skills_tab::SkillsTabState::default(),
            calcs_state: calcs_tab::CalcsTabState::default(),
            import_export_state: import_export_tab::ImportExportTabState::default(),
            notes_state: notes_tab::NotesTabState::default(),
            skills,
            bases,
            current_build_path: None,
            status_message: None,
            tree_versions,
            tree_version: default_version,
            data_root,
            last_compute_hash: 0,
        })
    }

    /// wasm32 build of `load_initial`: bundles a single tree (`3_25`) plus the
    /// item-base table via `include_str!`. Skills aren't bundled — the dataset
    /// is ~5 MB and the tree tab doesn't need them. Items / skills tabs will
    /// appear empty until a fetch-on-demand path lands.
    #[cfg(target_arch = "wasm32")]
    fn load_initial() -> Result<LoadedApp, String> {
        let tree_json = include_str!("../../../data/trees/3_25.json");
        let bases_json = include_str!("../../../data/bases.json");
        let tree = pob_data::load_passive_tree(tree_json)
            .map_err(|e| format!("parsing tree: {e}"))?;
        let bases = pob_data::load_bases(bases_json).ok();
        let skills = SkillRegistry::default();
        let sprites = load_sprite_metadata();
        let tree_view = TreeView::new(&tree, sprites.as_ref());
        let character = Character::new(ClassRef::marauder(), 1);
        let (output, env) = pob_engine::compute_full_with_env(
            &character,
            &tree,
            Some(&skills),
            bases.as_ref(),
        );
        Ok(LoadedApp {
            tree,
            tree_view,
            character,
            output,
            last_env: Some(env),
            search: String::new(),
            active_tab: Tab::Tree,
            items_state: items_tab::ItemsTabState::default(),
            skills_state: skills_tab::SkillsTabState::default(),
            calcs_state: calcs_tab::CalcsTabState::default(),
            import_export_state: import_export_tab::ImportExportTabState::default(),
            notes_state: notes_tab::NotesTabState::default(),
            skills,
            bases,
            current_build_path: None,
            status_message: None,
            tree_versions: vec!["3_25".to_owned()],
            tree_version: "3_25".to_owned(),
            data_root: PathBuf::from("/data"),
            last_compute_hash: 0,
        })
    }
}

/// Load the active + inactive skill atlases and decode them into RGBA8 byte
/// arrays the wgpu renderer can upload as textures. Returns `None` if the
/// assets aren't found or fail to decode — the renderer falls back to flat
/// colored circles in that case.
#[cfg(not(target_arch = "wasm32"))]
fn load_atlases() -> Option<tree_renderer::AtlasInputs> {
    use std::path::PathBuf;
    let candidates = [
        PathBuf::from("data/sprites/3_25"),
        PathBuf::from("../data/sprites/3_25"),
        PathBuf::from("../../data/sprites/3_25"),
    ];
    let dir = candidates.iter().find(|p| p.join("skills.jpg").is_file())?;
    let active = std::fs::read(dir.join("skills.jpg")).ok()?;
    let inactive = std::fs::read(dir.join("skills-disabled.jpg")).ok()?;
    let group = std::fs::read(dir.join("group-background.png")).ok()?;
    let frame = std::fs::read(dir.join("frame.png")).ok()?;
    let mastery = std::fs::read(dir.join("mastery.png")).ok()?;
    decode_atlas_inputs(&active, &inactive, &group, &frame, &mastery)
}

#[cfg(target_arch = "wasm32")]
fn load_atlases() -> Option<tree_renderer::AtlasInputs> {
    let active = include_bytes!("../../../data/sprites/3_25/skills.jpg");
    let inactive = include_bytes!("../../../data/sprites/3_25/skills-disabled.jpg");
    let group = include_bytes!("../../../data/sprites/3_25/group-background.png");
    let frame = include_bytes!("../../../data/sprites/3_25/frame.png");
    let mastery = include_bytes!("../../../data/sprites/3_25/mastery.png");
    decode_atlas_inputs(active, inactive, group, frame, mastery)
}

fn decode_atlas_inputs(
    active: &[u8],
    inactive: &[u8],
    group: &[u8],
    frame: &[u8],
    mastery: &[u8],
) -> Option<tree_renderer::AtlasInputs> {
    let active_img = image::load_from_memory(active).ok()?.to_rgba8();
    let inactive_img = image::load_from_memory(inactive).ok()?.to_rgba8();
    let group_img = image::load_from_memory(group).ok()?.to_rgba8();
    let frame_img = image::load_from_memory(frame).ok()?.to_rgba8();
    let mastery_img = image::load_from_memory(mastery).ok()?.to_rgba8();
    let active_size = (active_img.width(), active_img.height());
    let inactive_size = (inactive_img.width(), inactive_img.height());
    let group_size = (group_img.width(), group_img.height());
    let frame_size = (frame_img.width(), frame_img.height());
    let mastery_size = (mastery_img.width(), mastery_img.height());
    Some(tree_renderer::AtlasInputs {
        active_rgba8: active_img.into_raw(),
        active_size,
        inactive_rgba8: inactive_img.into_raw(),
        inactive_size,
        group_rgba8: group_img.into_raw(),
        group_size,
        frame_rgba8: frame_img.into_raw(),
        frame_size,
        mastery_rgba8: mastery_img.into_raw(),
        mastery_size,
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn load_sprite_metadata() -> Option<pob_data::sprites::SpriteSet> {
    use std::path::PathBuf;
    let candidates = [
        PathBuf::from("data/sprites/3_25.json"),
        PathBuf::from("../data/sprites/3_25.json"),
        PathBuf::from("../../data/sprites/3_25.json"),
    ];
    let path = candidates.iter().find(|p| p.is_file())?;
    let json = std::fs::read_to_string(path).ok()?;
    pob_data::sprites::load_sprites(&json).ok()
}

#[cfg(target_arch = "wasm32")]
fn load_sprite_metadata() -> Option<pob_data::sprites::SpriteSet> {
    let json = include_str!("../../../data/sprites/3_25.json");
    pob_data::sprites::load_sprites(json).ok()
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
    // Recompute is gated on actual input changes for performance — `compute_with_skills`
    // is fast enough at ~2ms/call but doing it every frame still wastes cycles.
    // Tab switches don't count as input changes, only modifications to the character
    // (class, level, allocated nodes, items, skills, config, notes).
    let mut recompute = false;

    // Keyboard shortcuts at the app level. egui::Key + Modifiers are tested in
    // `Context::input` so they fire regardless of which panel has focus (unless an
    // egui widget is consuming text input — e.g. the search box).
    let mut menu_action: Option<MenuAction> = None;
    let mut tab_jump: Option<Tab> = None;
    ctx.input(|i| {
        let cmd = i.modifiers.command;
        let shift = i.modifiers.shift;
        if cmd && i.key_pressed(egui::Key::S) {
            menu_action = Some(if shift { MenuAction::SaveAs } else { MenuAction::Save });
        } else if cmd && i.key_pressed(egui::Key::O) {
            menu_action = Some(MenuAction::Open);
        } else if cmd && i.key_pressed(egui::Key::N) {
            menu_action = Some(MenuAction::New);
        } else if cmd && i.key_pressed(egui::Key::Num1) {
            tab_jump = Some(Tab::Tree);
        } else if cmd && i.key_pressed(egui::Key::Num2) {
            tab_jump = Some(Tab::Items);
        } else if cmd && i.key_pressed(egui::Key::Num3) {
            tab_jump = Some(Tab::Skills);
        } else if cmd && i.key_pressed(egui::Key::Num4) {
            tab_jump = Some(Tab::Config);
        } else if cmd && i.key_pressed(egui::Key::Num5) {
            tab_jump = Some(Tab::Calcs);
        } else if cmd && i.key_pressed(egui::Key::Num6) {
            tab_jump = Some(Tab::Notes);
        } else if cmd && i.key_pressed(egui::Key::Num7) {
            tab_jump = Some(Tab::ImportExport);
        }
    });
    if let Some(action) = menu_action {
        apply_menu_action(app, action);
    }
    if let Some(t) = tab_jump {
        app.active_tab = t;
    }

    egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
        egui::menu::bar(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("New").clicked() {
                    apply_menu_action(app, MenuAction::New);
                    ui.close_menu();
                }
                if ui.button("Demo build (Witch / Arc)").clicked() {
                    apply_menu_action(app, MenuAction::DemoBuild);
                    ui.close_menu();
                }
                if ui.button("Open…").clicked() {
                    apply_menu_action(app, MenuAction::Open);
                    ui.close_menu();
                }
                let save_label = if app.current_build_path.is_some() { "Save" } else { "Save…" };
                if ui.button(save_label).clicked() {
                    apply_menu_action(app, MenuAction::Save);
                    ui.close_menu();
                }
                if ui.button("Save As…").clicked() {
                    apply_menu_action(app, MenuAction::SaveAs);
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Quit").clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.separator();
            if let Some(p) = &app.current_build_path {
                ui.weak(format!("{}", p.file_name().map(|n| n.to_string_lossy()).unwrap_or_default()));
            } else {
                ui.weak("(unsaved)");
            }
        });
    });

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
        .default_width(260.0)
        .min_width(220.0)
        .show(ctx, |ui| {
            ui.heading("Character");
            ui.separator();

            egui::ComboBox::from_label("Class")
                .selected_text(app.character.class.0.clone())
                .show_ui(ui, |ui| {
                    for c in &app.tree.classes {
                        if ui.selectable_label(app.character.class.0 == c.name, &c.name).clicked() {
                            app.character.class = ClassRef(c.name.clone());
                            // Reset ascendancy when class changes — old one no longer valid.
                            app.character.ascendancy = None;
                            // Drop any allocated ascendancy nodes that are no longer
                            // valid for the new class. We only filter by ascendancy
                            // tagging — non-ascendancy nodes stay.
                            app.character.allocated.retain(|id| {
                                app.tree
                                    .nodes
                                    .get(id)
                                    .map(|n| n.ascendancy_name.is_none())
                                    .unwrap_or(true)
                            });
                            recompute = true;
                        }
                    }
                });
            // Ascendancy options come from the selected class.
            if let Some(class) = app
                .tree
                .classes
                .iter()
                .find(|c| c.name == app.character.class.0)
            {
                let current = app.character.ascendancy.clone().unwrap_or_else(|| "(None)".into());
                egui::ComboBox::from_label("Ascendancy")
                    .selected_text(&current)
                    .show_ui(ui, |ui| {
                        if ui.selectable_label(app.character.ascendancy.is_none(), "(None)").clicked() {
                            app.character.ascendancy = None;
                            recompute = true;
                        }
                        for asc in &class.ascendancies {
                            let selected = app.character.ascendancy.as_deref() == Some(asc.id.as_str());
                            if ui.selectable_label(selected, &asc.id).clicked() {
                                app.character.ascendancy = Some(asc.id.clone());
                                recompute = true;
                            }
                        }
                    });
            }
            ui.add(
                egui::Slider::new(&mut app.character.level, 1..=100)
                    .text("Level")
                    .step_by(1.0),
            );
            if ui.button("Reset allocation").clicked() {
                app.character.allocated.clear();
                recompute = true;
            }
            let asc_alloc = app.character.ascendancy_alloc_count(&app.tree);
            let total_alloc = app.character.allocated.len() as u32;
            ui.label(format!(
                "Allocated: {} (passive) / {} / {} (ascendancy)",
                total_alloc - asc_alloc,
                asc_alloc,
                app.tree.points.ascendancy_points,
            ));
            let item_count = app.character.items.iter().count();
            if item_count > 0 {
                ui.label(format!("Items equipped: {item_count}"));
            }
            if app.character.main_skill.is_some() {
                ui.label(format!(
                    "Main skill: {}",
                    app.character.main_skill.as_ref().unwrap().skill_id
                ));
            }

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
            stat_row(ui, "Fire Res", &app.output, "FireResistTotal");
            stat_row(ui, "Cold Res", &app.output, "ColdResistTotal");
            stat_row(ui, "Lightning Res", &app.output, "LightningResistTotal");
            stat_row(ui, "Chaos Res", &app.output, "ChaosResistTotal");
            ui.add_space(4.0);
            stat_row(ui, "Armour", &app.output, "Armour");
            stat_row_decimal(ui, "Phys reduction %", &app.output, "PhysicalDamageReduction");
            stat_row(ui, "Evasion", &app.output, "Evasion");
            stat_row(ui, "Block", &app.output, "BlockChance");
            stat_row(ui, "Spell Block", &app.output, "SpellBlockChance");
            stat_row(ui, "Spell Suppress", &app.output, "SpellSuppressionChance");
            ui.add_space(4.0);
            stat_row_decimal(ui, "Life regen", &app.output, "LifeRegen");
            stat_row_decimal(ui, "Mana regen", &app.output, "ManaRegen");
            ui.add_space(4.0);
            stat_row(ui, "EHP (avg)", &app.output, "AverageEHP");

            ui.add_space(6.0);
            ui.collapsing("Per-element defence", |ui| {
                ui.weak("EHP and max-hit-taken split by damage type:");
                ui.add_space(2.0);
                stat_row(ui, "Phys EHP", &app.output, "PhysicalEHP");
                stat_row(ui, "Fire EHP", &app.output, "FireEHP");
                stat_row(ui, "Cold EHP", &app.output, "ColdEHP");
                stat_row(ui, "Lightning EHP", &app.output, "LightningEHP");
                stat_row(ui, "Chaos EHP", &app.output, "ChaosEHP");
                ui.add_space(4.0);
                stat_row(ui, "Min EHP", &app.output, "MinimumEHP");
                stat_row(ui, "Total EHP", &app.output, "TotalEHP");
                ui.add_space(4.0);
                stat_row(ui, "Phys max hit", &app.output, "PhysicalMaximumHitTaken");
                stat_row(ui, "Fire max hit", &app.output, "FireMaximumHitTaken");
                stat_row(ui, "Cold max hit", &app.output, "ColdMaximumHitTaken");
                stat_row(ui, "Lightning max hit", &app.output, "LightningMaximumHitTaken");
                stat_row(ui, "Chaos max hit", &app.output, "ChaosMaximumHitTaken");
            });

            ui.add_space(8.0);
            ui.heading("Main skill");
            ui.separator();
            if app.character.main_skill.is_some() {
                if app.output.get("MainSkillIsDotOnly") > 0.0 {
                    ui.colored_label(
                        egui::Color32::LIGHT_YELLOW,
                        "DoT-only skill — hit DPS below is not meaningful",
                    );
                }
                stat_row_decimal(ui, "Avg hit", &app.output, "MainSkillAverageHit");
                stat_row_decimal(ui, "Crit chance %", &app.output, "MainSkillCritChance");
                stat_row_decimal(ui, "Avg w/ crit", &app.output, "MainSkillAverageHitWithCrit");
                stat_row_decimal(ui, "Hit chance %", &app.output, "MainSkillHitChance");
                stat_row_decimal(ui, "Speed (cps)", &app.output, "MainSkillSpeed");
                stat_row_decimal(ui, "DPS", &app.output, "MainSkillDPS");
                if app.output.get("ManaPerSecondCost") > 0.0 {
                    stat_row_decimal(ui, "Mana / sec", &app.output, "ManaPerSecondCost");
                } else if app.output.get("MainSkillManaCost") > 0.0 {
                    stat_row_decimal(ui, "Mana cost", &app.output, "MainSkillManaCost");
                }
                if app.output.get("ChainMax") > 0.0 {
                    stat_row(ui, "Chain count", &app.output, "ChainMax");
                }
                if app.output.get("BleedDPS") > 0.0 {
                    stat_row_decimal(ui, "Bleed DPS", &app.output, "BleedDPS");
                }
                if app.output.get("PoisonDPS") > 0.0 {
                    stat_row_decimal(ui, "Poison DPS", &app.output, "PoisonDPS");
                }
                if app.output.get("IgniteDPS") > 0.0 {
                    stat_row_decimal(ui, "Ignite DPS", &app.output, "IgniteDPS");
                }
                stat_row_decimal(ui, "Full DPS", &app.output, "FullDPS");
            } else if ui.link("Pick a skill →").clicked() {
                app.active_tab = Tab::Skills;
            }
        });

    egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
        ui.horizontal(|ui| {
            let mut new_version: Option<String> = None;
            egui::ComboBox::from_label("Tree version")
                .selected_text(app.tree_version.as_str())
                .show_ui(ui, |ui| {
                    for v in &app.tree_versions {
                        if ui
                            .selectable_label(*v == app.tree_version, v)
                            .clicked()
                        {
                            new_version = Some(v.clone());
                        }
                    }
                });
            if let Some(v) = new_version {
                if let Err(e) = swap_tree(app, &v) {
                    app.status_message = Some((StatusKind::Error, e));
                }
            }
            ui.separator();
            ui.label(format!(
                "{} nodes ({} groups)",
                app.tree.nodes.len(),
                app.tree.groups.len()
            ));
            ui.separator();
            if app.active_tab == Tab::Tree {
                ui.label(format!("Zoom: {:.3}", app.tree_view.zoom()));
                ui.separator();
                ui.label("Drag to pan, scroll to zoom, click to allocate.");
            } else if let Some((kind, msg)) = &app.status_message {
                let colour = match kind {
                    StatusKind::Info => egui::Color32::LIGHT_GREEN,
                    StatusKind::Error => egui::Color32::LIGHT_RED,
                };
                ui.colored_label(colour, msg);
            }
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
                // Block ascendancy nodes that don't belong to the selected ascendancy.
                let node = app.tree.nodes.get(&id);
                let allowed_ascend = node
                    .map(|n| {
                        n.ascendancy_name.is_none()
                            || n.ascendancy_name.as_deref() == app.character.ascendancy.as_deref()
                    })
                    .unwrap_or(true);
                let toggling_off = app.character.allocated.contains(&id);

                if !allowed_ascend {
                    app.status_message = Some((
                        StatusKind::Error,
                        "Node belongs to a different ascendancy class.".into(),
                    ));
                } else if toggling_off {
                    app.character.allocated.remove(&id);
                    recompute = true;
                } else {
                    // Unallocated click: allocate the whole shortest path from any
                    // already-allocated node to the target. Mirrors PoB / poeplanner's
                    // "click an outlying notable to jump there" behavior. Falls back
                    // to a single-node insert when there's no allocated set yet
                    // (first click) or pathfinding fails (e.g. behind an unreachable
                    // ascendancy gate).
                    let allocated_set: std::collections::HashSet<NodeId> =
                        app.character.allocated.iter().copied().collect();
                    let path_opt = if allocated_set.is_empty() {
                        Some(vec![id])
                    } else {
                        pathfind::shortest_path_from_allocated(&app.tree, &allocated_set, id)
                    };
                    if let Some(path) = path_opt {
                        // The first entry of `path` is an already-allocated root
                        // (or `id` itself when allocated is empty); skip that.
                        let first_idx = if allocated_set.is_empty() { 0 } else { 1 };
                        let new_ascend_in_path: u32 = path[first_idx..]
                            .iter()
                            .filter(|nid| {
                                app.tree
                                    .nodes
                                    .get(nid)
                                    .and_then(|n| n.ascendancy_name.as_deref())
                                    .is_some()
                            })
                            .count() as u32;
                        let already_ascend = app.character.ascendancy_alloc_count(&app.tree);
                        let budget = app.tree.points.ascendancy_points;
                        if new_ascend_in_path > 0
                            && already_ascend + new_ascend_in_path > budget
                        {
                            app.status_message = Some((
                                StatusKind::Error,
                                format!(
                                    "Path would exceed the {budget}-point ascendancy budget."
                                ),
                            ));
                        } else {
                            for nid in &path[first_idx..] {
                                app.character.allocated.insert(*nid);
                            }
                            recompute = true;
                        }
                    } else {
                        app.status_message = Some((
                            StatusKind::Error,
                            "Cannot reach that node from the currently allocated set.".into(),
                        ));
                    }
                }
            }
        }
        Tab::Items => {
            if items_tab::ui(ui, &mut app.items_state, &mut app.character.items) {
                recompute = true;
            }
        }
        Tab::Skills => {
            if skills_tab::ui(ui, &mut app.skills_state, &mut app.character, &app.skills) {
                recompute = true;
            }
        }
        Tab::Config => {
            if config_tab::ui(ui, &mut app.character.config) {
                recompute = true;
            }
        }
        Tab::Calcs => {
            calcs_tab::ui(
                ui,
                &mut app.calcs_state,
                &app.output,
                app.last_env.as_ref(),
            );
        }
        Tab::Notes => {
            notes_tab::ui(ui, &mut app.character.notes, &mut app.notes_state);
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

    // Fingerprint the inputs that compute() actually depends on so we don't run the
    // full pipeline (~5ms in release) on every frame regardless of whether anything
    // changed. The recompute flag forces a re-run after explicit user edits.
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    app.character.class.0.hash(&mut hasher);
    app.character.ascendancy.hash(&mut hasher);
    app.character.level.hash(&mut hasher);
    let mut alloc_sorted: Vec<_> = app.character.allocated.iter().copied().collect();
    alloc_sorted.sort_unstable();
    alloc_sorted.hash(&mut hasher);
    app.tree_version.hash(&mut hasher);
    if let Some(m) = &app.character.main_skill {
        m.skill_id.hash(&mut hasher);
        m.level.hash(&mut hasher);
        m.quality.hash(&mut hasher);
    }
    let mut conds: Vec<_> = app.character.config.conditions.iter().collect();
    conds.sort_by(|a, b| a.0.cmp(b.0));
    for (k, v) in conds {
        k.hash(&mut hasher);
        v.hash(&mut hasher);
    }
    let mut mults: Vec<_> = app.character.config.multipliers.iter().collect();
    mults.sort_by(|a, b| a.0.cmp(b.0));
    for (k, v) in mults {
        k.hash(&mut hasher);
        v.to_bits().hash(&mut hasher);
    }
    app.character.config.enemy_level.hash(&mut hasher);
    app.character.config.enemy_fire_resist.hash(&mut hasher);
    app.character.config.enemy_cold_resist.hash(&mut hasher);
    app.character.config.enemy_lightning_resist.hash(&mut hasher);
    app.character.config.enemy_chaos_resist.hash(&mut hasher);
    app.character.config.enemy_evasion.hash(&mut hasher);
    let item_count = app.character.items.iter().count();
    item_count.hash(&mut hasher);
    for (slot, item) in app.character.items.iter() {
        format!("{slot:?}").hash(&mut hasher);
        item.base_name.hash(&mut hasher);
        item.mod_lines.len().hash(&mut hasher);
        for ml in &item.mod_lines {
            ml.line.hash(&mut hasher);
        }
    }
    let h = hasher.finish();

    if recompute || h != app.last_compute_hash {
        app.last_compute_hash = h;
        let (output, env) = pob_engine::compute_full_with_env(
            &app.character,
            &app.tree,
            Some(&app.skills),
            app.bases.as_ref(),
        );
        app.output = output;
        app.last_env = Some(env);
    }
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

#[derive(Debug, Clone, Copy)]
enum MenuAction {
    New,
    DemoBuild,
    Open,
    Save,
    SaveAs,
}

fn apply_menu_action(app: &mut LoadedApp, action: MenuAction) {
    match action {
        MenuAction::New => {
            app.character = Character::new(ClassRef::marauder(), 1);
            app.current_build_path = None;
            app.status_message = Some((StatusKind::Info, "New build.".into()));
        }
        MenuAction::DemoBuild => {
            // A non-trivial Witch / Occultist with Arc to demo the full calc pipeline
            // without making the user paste anything.
            let mut c = Character::new(ClassRef::witch(), 90);
            c.ascendancy = Some("Occultist".into());
            // Populate via the socket-group model so the Skills tab shows the
            // bound gem and the user can swap it / add supports.
            c.skill_groups.push(pob_engine::character::SocketGroup {
                label: "Main".into(),
                gems: vec![pob_engine::MainSkill {
                    skill_id: "Arc".into(),
                    level: 20,
                    quality: 20,
                    enabled: true,
                }],
                main_active_skill_index: 1,
                enabled: true,
            });
            c.main_socket_group = 1;
            c.sync_main_skill();
            c.config.enemy_lightning_resist = 50;
            // Equip a basic resist amulet so resists move.
            let amulet_paste =
                "Item Class: Amulets\nRarity: RARE\nDemo Charm\nOnyx Amulet\n--------\n+10 to all Attributes\n+62 to maximum Life\n+39% to all Elemental Resistances\n--------";
            if let Ok(item) = pob_engine::parse_item(amulet_paste) {
                c.items.equip(pob_data::Slot::Amulet, item);
            }
            app.character = c;
            app.current_build_path = None;
            app.status_message = Some((
                StatusKind::Info,
                "Demo build loaded: Witch L90 Occultist Arc.".into(),
            ));
        }
        MenuAction::Open => {
            #[cfg(not(target_arch = "wasm32"))]
            {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Build file", &["mk2", "xml"])
                    .add_filter("MK2 build", &["mk2"])
                    .add_filter("PoB XML", &["xml"])
                    .pick_file()
                {
                    let load = std::fs::read_to_string(&path).map_err(|e| e.to_string());
                    let parse_result: Result<Character, String> = load.and_then(|s| {
                        let trimmed = s.trim();
                        // Auto-detect: MK2 codes start with "MK2|", XML starts with "<".
                        if trimmed.starts_with("MK2|") {
                            pob_engine::import_code(trimmed).map_err(|e| e.to_string())
                        } else if trimmed.starts_with('<') {
                            pob_engine::import_pob_xml(trimmed).map_err(|e| e.to_string())
                        } else {
                            // Maybe a PoB share-code (zlib+base64) saved to file.
                            pob_engine::import_pob_code(trimmed).map_err(|e| e.to_string())
                        }
                    });
                    match parse_result {
                        Ok(c) => {
                            app.character = c;
                            app.current_build_path = Some(path.clone());
                            app.status_message = Some((
                                StatusKind::Info,
                                format!("Opened {}", path.display()),
                            ));
                        }
                        Err(e) => {
                            app.status_message =
                                Some((StatusKind::Error, format!("Open failed: {e}")));
                        }
                    }
                }
            }
            #[cfg(target_arch = "wasm32")]
            {
                app.status_message = Some((
                    StatusKind::Error,
                    "Open is not supported in the web build yet — paste a build via Import/Export.".into(),
                ));
            }
        }
        MenuAction::Save => save_build(app, false),
        MenuAction::SaveAs => save_build(app, true),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn swap_tree(app: &mut LoadedApp, version: &str) -> Result<(), String> {
    let path = app
        .data_root
        .join("trees")
        .join(format!("{version}.json"));
    let json = std::fs::read_to_string(&path).map_err(|e| format!("reading {path:?}: {e}"))?;
    let tree = pob_data::load_passive_tree(&json).map_err(|e| format!("parse: {e}"))?;
    app.tree_version = version.to_owned();
    app.tree = tree;
    app.tree_view.rebind(&app.tree);
    app.status_message = Some((StatusKind::Info, format!("Loaded tree {version}.")));
    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn swap_tree(_app: &mut LoadedApp, _version: &str) -> Result<(), String> {
    // The web build only bundles `3_25.json`, so there's nothing to swap to.
    Err("Tree-version switching is disabled in the web build (only 3_25 bundled).".into())
}

#[cfg(not(target_arch = "wasm32"))]
fn save_build(app: &mut LoadedApp, force_dialog: bool) {
    let path = if force_dialog || app.current_build_path.is_none() {
        rfd::FileDialog::new()
            .add_filter("MK2 build", &["mk2"])
            .add_filter("PoB XML", &["xml"])
            .set_file_name("build.mk2")
            .save_file()
    } else {
        app.current_build_path.clone()
    };
    let Some(path) = path else {
        return;
    };
    // Pick the format from the file extension. .xml emits a PoB-compatible
    // document so users can round-trip into upstream PoB; everything else
    // gets the compact MK2 share code.
    let is_xml = path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.eq_ignore_ascii_case("xml"))
        .unwrap_or(false);
    let payload = if is_xml {
        Ok(pob_engine::export_pob_xml(&app.character))
    } else {
        pob_engine::export_code(&app.character).map_err(|e| e.to_string())
    };
    match payload.and_then(|code| std::fs::write(&path, code).map_err(|e| e.to_string())) {
        Ok(()) => {
            app.current_build_path = Some(path.clone());
            app.status_message = Some((
                StatusKind::Info,
                format!("Saved to {}", path.display()),
            ));
        }
        Err(e) => {
            app.status_message = Some((StatusKind::Error, format!("Save failed: {e}")));
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn save_build(app: &mut LoadedApp, _force_dialog: bool) {
    app.status_message = Some((
        StatusKind::Error,
        "Save is not supported in the web build yet — copy from Import/Export.".into(),
    ));
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
