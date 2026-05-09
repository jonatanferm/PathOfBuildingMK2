//! egui UI for Path of Building MK2. Phase 4a: passive tree screen with live stats.

use std::collections::HashSet;
use std::path::PathBuf;

use eframe::egui;
use pob_data::{NodeId, PassiveTree};
use pob_engine::{
    character::{Bandit, ClassRef, MajorGod, MinorGod},
    Character, Output, SkillRegistry,
};

#[cfg(not(target_arch = "wasm32"))]
mod build_store_disk;
#[cfg(target_arch = "wasm32")]
mod build_store_wasm;
mod builds_tab;
mod calcs_tab;
mod color_codes;
mod compare_tab;
mod config_tab;
mod import_export_tab;
mod items_tab;
mod notes_tab;
mod party_tab;
mod skills_tab;
mod tree_layout;
mod tree_renderer;
mod tree_view;

use pob_engine::pathfind;

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
    compare_state: compare_tab::CompareTabState,
    party_state: party_tab::PartyTabState,
    builds_state: builds_tab::BuildsTabState,
    import_export_state: import_export_tab::ImportExportTabState,
    notes_state: notes_tab::NotesTabState,
    skills: SkillRegistry,
    bases: Option<pob_data::bases::ItemBaseSet>,
    /// Issue #110: cached sprite metadata so the per-frame class
    /// portrait gating can call `tree_view.set_active_class` on
    /// class changes without re-reading `sprite_atlases.json` from
    /// disk each time.
    sprites: Option<pob_data::sprites::SpriteSet>,
    /// Issue #34 (slice 1+2): the imported PoB Calcs-tab section layout.
    /// `None` if `data/calc_sections.json` is missing — the Calcs tab
    /// silently falls back to its legacy flat-key view in that case.
    calc_sections: Option<Vec<pob_data::CalcSection>>,
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
    /// Issue #100 (slice 1): auto-save bookkeeping. `last_saved_hash`
    /// tracks the hash of the build state that's currently on disk;
    /// `dirty_since` is set when `last_compute_hash` diverges from
    /// `last_saved_hash`, and cleared after a successful auto-save.
    /// On native targets, when `dirty_since.elapsed() > 2s` and a
    /// `current_build_path` is set, the frame loop writes the build
    /// to disk without prompting. wasm builds skip this entirely.
    last_saved_hash: u64,
    #[cfg(not(target_arch = "wasm32"))]
    dirty_since: Option<std::time::Instant>,
    /// Set by load paths so the next compute pass seeds
    /// `last_saved_hash = last_compute_hash` (i.e. mark the
    /// freshly-loaded build as already on disk, suppressing the
    /// no-op auto-save that would otherwise fire next tick).
    pending_seed_saved_hash: bool,
    /// Issue #101: wasm-only storage shim backing the Builds tab with
    /// IndexedDB (and optionally a File System Access folder).
    /// Drained each frame for completed async ops.
    #[cfg(target_arch = "wasm32")]
    wasm_storage: build_store_wasm::WasmStorage,
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
    Compare,
    Party,
    Builds,
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
        let mut tree_versions: Vec<String> = pob_data::load_tree_index(&index_json)
            .map_err(|e| format!("parsing tree index: {e}"))?;
        tree_versions.sort();
        let default_version = tree_versions
            .iter()
            .filter(|v| !v.contains("alternate") && !v.contains("ruthless"))
            .next_back()
            .cloned()
            .or_else(|| tree_versions.last().cloned())
            .ok_or_else(|| "no tree versions found".to_owned())?;
        let tree_path = data_root
            .join("trees")
            .join(format!("{default_version}.json"));
        let tree_json =
            std::fs::read_to_string(&tree_path).map_err(|e| format!("reading tree: {e}"))?;
        let tree =
            pob_data::load_passive_tree(&tree_json).map_err(|e| format!("parsing tree: {e}"))?;

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
        let calc_sections = std::fs::read_to_string(data_root.join("calc_sections.json"))
            .ok()
            .and_then(|json| pob_data::load_calc_sections(&json).ok());
        let mut tree_view = TreeView::new(&tree, sprites.as_ref());
        let character = Character::new(ClassRef::marauder(), 1);
        // Issue #110: gate the class portrait sprites on the active
        // class up-front so the initial render shows only the
        // Marauder portrait (other six fall back to the inactive
        // background).
        tree_view.set_active_class(Some(&character.class.0), &tree, sprites.as_ref());
        let (output, env) =
            pob_engine::compute_full_with_env(&character, &tree, Some(&skills), bases.as_ref());

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
            compare_state: compare_tab::CompareTabState::default(),
            party_state: party_tab::PartyTabState::default(),
            builds_state: builds_tab::BuildsTabState::default(),
            import_export_state: import_export_tab::ImportExportTabState::default(),
            notes_state: notes_tab::NotesTabState::default(),
            skills,
            bases,
            sprites,
            calc_sections,
            current_build_path: None,
            status_message: None,
            tree_versions,
            tree_version: default_version,
            data_root,
            last_compute_hash: 0,
            last_saved_hash: 0,
            dirty_since: None,
            pending_seed_saved_hash: false,
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
        let calc_sections_json = include_str!("../../../data/calc_sections.json");
        let tree =
            pob_data::load_passive_tree(tree_json).map_err(|e| format!("parsing tree: {e}"))?;
        let bases = pob_data::load_bases(bases_json).ok();
        let calc_sections = pob_data::load_calc_sections(calc_sections_json).ok();
        let skills = SkillRegistry::default();
        let sprites = load_sprite_metadata();
        let mut tree_view = TreeView::new(&tree, sprites.as_ref());
        let character = Character::new(ClassRef::marauder(), 1);
        tree_view.set_active_class(Some(&character.class.0), &tree, sprites.as_ref());
        let (output, env) =
            pob_engine::compute_full_with_env(&character, &tree, Some(&skills), bases.as_ref());
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
            compare_state: compare_tab::CompareTabState::default(),
            party_state: party_tab::PartyTabState::default(),
            builds_state: builds_tab::BuildsTabState::default(),
            import_export_state: import_export_tab::ImportExportTabState::default(),
            notes_state: notes_tab::NotesTabState::default(),
            skills,
            bases,
            sprites,
            calc_sections,
            current_build_path: None,
            status_message: None,
            tree_versions: vec!["3_25".to_owned()],
            tree_version: "3_25".to_owned(),
            data_root: PathBuf::from("/data"),
            last_compute_hash: 0,
            last_saved_hash: 0,
            pending_seed_saved_hash: false,
            wasm_storage: build_store_wasm::WasmStorage::new(),
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

    // Issue #101: drain async storage events first thing each frame
    // so list refreshes / load completions / status toasts surface
    // before any UI rendering touches them.
    #[cfg(target_arch = "wasm32")]
    {
        if apply_storage_events(app) {
            recompute = true;
            ctx.request_repaint();
        }
    }

    // Keyboard shortcuts at the app level. egui::Key + Modifiers are tested in
    // `Context::input` so they fire regardless of which panel has focus (unless an
    // egui widget is consuming text input — e.g. the search box).
    let mut menu_action: Option<MenuAction> = None;
    let mut tab_jump: Option<Tab> = None;
    ctx.input(|i| {
        let cmd = i.modifiers.command;
        let shift = i.modifiers.shift;
        if cmd && i.key_pressed(egui::Key::S) {
            menu_action = Some(if shift {
                MenuAction::SaveAs
            } else {
                MenuAction::Save
            });
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
                let save_label = if app.current_build_path.is_some() {
                    "Save"
                } else {
                    "Save…"
                };
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
                ui.weak(format!(
                    "{}",
                    p.file_name()
                        .map(|n| n.to_string_lossy())
                        .unwrap_or_default()
                ));
            } else {
                ui.weak("(unsaved)");
            }
            // Issue #100: dirty / saved indicator. Mirrors PoB's
            // header chrome — users want at-a-glance confirmation
            // their last edit hit disk before they kill the process.
            // Only meaningful once a save target exists; an untitled
            // scratch build shows the "(unsaved)" path label instead.
            #[cfg(not(target_arch = "wasm32"))]
            {
                if app.current_build_path.is_some() {
                    if app.dirty_since.is_some() {
                        ui.colored_label(egui::Color32::from_rgb(0xFF, 0x99, 0x22), "● Modified");
                    } else {
                        ui.colored_label(egui::Color32::from_rgb(0x33, 0xFF, 0x77), "✔ Saved");
                    }
                }
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
            ui.selectable_value(&mut app.active_tab, Tab::Compare, "Compare");
            ui.selectable_value(&mut app.active_tab, Tab::Party, "Party");
            ui.selectable_value(&mut app.active_tab, Tab::Builds, "Builds");
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
                        if ui
                            .selectable_label(app.character.class.0 == c.name, &c.name)
                            .clicked()
                        {
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
                let current = app
                    .character
                    .ascendancy
                    .clone()
                    .unwrap_or_else(|| "(None)".into());
                egui::ComboBox::from_label("Ascendancy")
                    .selected_text(&current)
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_label(app.character.ascendancy.is_none(), "(None)")
                            .clicked()
                        {
                            app.character.ascendancy = None;
                            recompute = true;
                        }
                        for asc in &class.ascendancies {
                            let selected =
                                app.character.ascendancy.as_deref() == Some(asc.id.as_str());
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
            // Issue #54: Bandit (Act 2 reward) selector. Mirrors PoB's
            // dropdown — Kill All grants +2 passive points (default), the
            // named bandits each grant a single hard-coded mod (Alira:
            // +15 to all elemental resistances; Kraityn: +8% movement
            // speed; Oak: +40 max life).
            let bandit_options: &[(Bandit, &str)] = &[
                (Bandit::KillAll, "Kill All (+2 passives)"),
                (Bandit::Alira, "Alira (+15% all-ele resists)"),
                (Bandit::Kraityn, "Kraityn (+8% move speed)"),
                (Bandit::Oak, "Oak (+40 life)"),
            ];
            let current_label = bandit_options
                .iter()
                .find(|(b, _)| *b == app.character.bandit)
                .map(|(_, l)| *l)
                .unwrap_or("Kill All");
            egui::ComboBox::from_label("Bandit")
                .selected_text(current_label)
                .show_ui(ui, |ui| {
                    for (option, label) in bandit_options {
                        if ui
                            .selectable_label(app.character.bandit == *option, *label)
                            .clicked()
                        {
                            app.character.bandit = *option;
                            recompute = true;
                        }
                    }
                });
            // Issue #10 (Pantheon half): Major + Minor god selectors.
            // Each god injects its soul[1] mod (defensive, mostly) at
            // compute time via `apply_pantheon_mods`.
            let major_options: &[MajorGod] = &[
                MajorGod::None,
                MajorGod::TheBrineKing,
                MajorGod::Arakaali,
                MajorGod::Solaris,
                MajorGod::Lunaris,
            ];
            egui::ComboBox::from_label("Major God")
                .selected_text(app.character.pantheon_major.display())
                .show_ui(ui, |ui| {
                    for option in major_options {
                        if ui
                            .selectable_label(
                                app.character.pantheon_major == *option,
                                option.display(),
                            )
                            .clicked()
                            && app.character.pantheon_major != *option
                        {
                            app.character.pantheon_major = *option;
                            recompute = true;
                        }
                    }
                });
            let minor_options: &[MinorGod] = &[
                MinorGod::None,
                MinorGod::Abberath,
                MinorGod::Gruthkul,
                MinorGod::Yugul,
                MinorGod::Shakari,
                MinorGod::Tukohama,
                MinorGod::Ralakesh,
                MinorGod::Garukhan,
                MinorGod::Ryslatha,
            ];
            egui::ComboBox::from_label("Minor God")
                .selected_text(app.character.pantheon_minor.display())
                .show_ui(ui, |ui| {
                    for option in minor_options {
                        if ui
                            .selectable_label(
                                app.character.pantheon_minor == *option,
                                option.display(),
                            )
                            .clicked()
                            && app.character.pantheon_minor != *option
                        {
                            app.character.pantheon_minor = *option;
                            recompute = true;
                        }
                    }
                });
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
            if let Some(main_skill) = app.character.main_skill.as_ref() {
                ui.label(format!("Main skill: {}", main_skill.skill_id));
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
            stat_row_decimal(
                ui,
                "Phys reduction %",
                &app.output,
                "PhysicalDamageReduction",
            );
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
                stat_row(
                    ui,
                    "Lightning max hit",
                    &app.output,
                    "LightningMaximumHitTaken",
                );
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
                stat_row_decimal(
                    ui,
                    "Avg w/ crit",
                    &app.output,
                    "MainSkillAverageHitWithCrit",
                );
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
                        if ui.selectable_label(*v == app.tree_version, v).clicked() {
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
            // shortest path from any allocated node (or the class-start anchor) to it.
            // Using `pathfind_seeds` keeps the preview in sync with `allocate_path`'s
            // anchor-aware behaviour, so a fresh Marauder sees the path from the start.
            app.tree_view.path_overlay.clear();
            if let Some(hover) = interaction.hovered {
                if !allocated.contains(&hover) {
                    let seeds = app.character.pathfind_seeds(&app.tree);
                    if !seeds.is_empty() {
                        if let Some(path) =
                            pathfind::shortest_path_from_allocated(&app.tree, &seeds, hover)
                        {
                            app.tree_view.path_overlay = path;
                        }
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
                    // Unallocate: removes the clicked node *and* any nodes that
                    // are now disconnected from the class start.
                    app.character.unallocate(&app.tree, id);
                    recompute = true;
                } else {
                    // Unallocated click: allocate the whole shortest path from any
                    // already-allocated node (or the class-start anchor) to the target.
                    // Mirrors PoB / poeplanner's "click an outlying notable to jump
                    // there" behavior. Falls back to a single-node insert only when
                    // there are no seeds at all — i.e. no class set and nothing
                    // allocated.
                    let seeds = app.character.pathfind_seeds(&app.tree);
                    let path_opt = if seeds.is_empty() {
                        Some(vec![id])
                    } else {
                        pathfind::shortest_path_from_allocated(&app.tree, &seeds, id)
                    };
                    if let Some(path) = path_opt {
                        // Path[0] is a seed (real allocation or virtual anchor) — or
                        // `id` itself in the no-seeds fallback. Skip it for the
                        // budget check so we only count nodes we'd actually allocate.
                        let first_idx = if seeds.is_empty() { 0 } else { 1 };
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
                        if new_ascend_in_path > 0 && already_ascend + new_ascend_in_path > budget {
                            app.status_message = Some((
                                StatusKind::Error,
                                format!("Path would exceed the {budget}-point ascendancy budget."),
                            ));
                        } else {
                            app.character.allocate_path(&app.tree, id);
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
            if items_tab::ui(ui, &mut app.items_state, &mut app.character) {
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
                app.calc_sections.as_deref(),
            );
        }
        Tab::Compare => {
            if let Some(action) =
                compare_tab::ui(ui, &mut app.compare_state, &app.character, &app.output)
            {
                handle_compare_action(app, action);
            }
        }
        Tab::Party => {
            if party_tab::ui(
                ui,
                &mut app.party_state,
                &mut app.character,
                &app.skills,
                &app.tree,
                app.bases.as_ref(),
            ) {
                recompute = true;
            }
        }
        Tab::Builds => {
            if let Some(action) = builds_tab::ui(ui, &mut app.builds_state) {
                handle_builds_action(app, action);
                recompute = true;
            }
        }
        Tab::Notes => {
            notes_tab::ui(ui, &mut app.character.notes, &mut app.notes_state);
        }
        Tab::ImportExport => {
            if import_export_tab::ui(ui, &mut app.import_export_state, &mut app.character) {
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
    app.character
        .config
        .enemy_lightning_resist
        .hash(&mut hasher);
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
    // Issue #110: re-gate the class portrait sprites whenever the
    // active class might have changed. `set_active_class` is a no-op
    // when the class index hasn't actually changed since the last
    // call, so calling it every frame is cheap.
    app.tree_view.set_active_class(
        Some(&app.character.class.0),
        &app.tree,
        app.sprites.as_ref(),
    );
    // Issue #100: track dirty state relative to what's currently on
    // disk. When the recomputed input hash diverges from the
    // last-saved hash, mark the build dirty (unless we already are);
    // when they match again (e.g. after auto-save or manual save),
    // clear the dirty mark. wasm has no disk so the field gates out.
    if app.pending_seed_saved_hash {
        app.last_saved_hash = app.last_compute_hash;
        app.pending_seed_saved_hash = false;
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        if app.last_compute_hash != app.last_saved_hash {
            if app.dirty_since.is_none() {
                app.dirty_since = Some(std::time::Instant::now());
            }
        } else {
            app.dirty_since = None;
        }
        try_autosave(app);
    }
}

/// Issue #100 (slice 1): auto-save the build to its current path
/// once it has been dirty for at least `IDLE_AUTOSAVE_MS`. Mirrors
/// PoB's `Modules/Build.lua::SaveDBFile` cadence — saves on a debounce,
/// not on every change. Skips silently when no path is set (unnamed
/// scratch builds need an explicit Save As first).
#[cfg(not(target_arch = "wasm32"))]
fn try_autosave(app: &mut LoadedApp) {
    const IDLE_AUTOSAVE_MS: u128 = 2000;
    let Some(since) = app.dirty_since else {
        return;
    };
    if since.elapsed().as_millis() < IDLE_AUTOSAVE_MS {
        return;
    }
    let Some(path) = app.current_build_path.clone() else {
        // Untitled buffer — leave dirty until the user picks a path.
        return;
    };
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
            app.last_saved_hash = app.last_compute_hash;
            app.dirty_since = None;
            app.status_message = Some((
                StatusKind::Info,
                format!("Auto-saved to {}", path.display()),
            ));
        }
        Err(e) => {
            // Don't clear `dirty_since` — we'll retry on the next idle
            // tick; surfacing the error keeps the user informed.
            app.status_message = Some((StatusKind::Error, format!("Auto-save failed: {e}")));
            app.dirty_since = Some(std::time::Instant::now());
        }
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
                            // Just loaded → matches what's on disk.
                            // Defer seeding `last_saved_hash` to the next
                            // compute (when the new input hash exists),
                            // so auto-save doesn't fire on a no-op write.
                            app.pending_seed_saved_hash = true;
                            app.dirty_since = None;
                            app.status_message =
                                Some((StatusKind::Info, format!("Opened {}", path.display())));
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
                // Web build: re-use the Builds tab's import-file path
                // (file picker → parse → store in IDB / folder).
                app.wasm_storage
                    .handle_action(builds_tab::BuildsAction::ImportFile, &app.character);
            }
        }
        MenuAction::Save => save_build(app, false),
        MenuAction::SaveAs => save_build(app, true),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn swap_tree(app: &mut LoadedApp, version: &str) -> Result<(), String> {
    let path = app.data_root.join("trees").join(format!("{version}.json"));
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

/// Handle a Compare-tab action — currently only "load comparison from
/// disk." Picks a file, imports it, runs `compute_full` against the
/// imported character with the host's tree / skills / bases, and
/// installs the resulting (character, output) pair as the snapshot.
#[cfg(not(target_arch = "wasm32"))]
fn handle_compare_action(app: &mut LoadedApp, action: compare_tab::CompareAction) {
    match action {
        compare_tab::CompareAction::LoadFromFile => {
            let Some(path) = rfd::FileDialog::new()
                .add_filter("Build file", &["mk2", "xml"])
                .add_filter("MK2 build", &["mk2"])
                .add_filter("PoB XML", &["xml"])
                .pick_file()
            else {
                return;
            };
            let parsed: Result<Character, String> = std::fs::read_to_string(&path)
                .map_err(|e| e.to_string())
                .and_then(|s| {
                    let trimmed = s.trim();
                    if trimmed.starts_with("MK2|") {
                        pob_engine::import_code(trimmed).map_err(|e| e.to_string())
                    } else if trimmed.starts_with('<') {
                        pob_engine::import_pob_xml(trimmed).map_err(|e| e.to_string())
                    } else {
                        pob_engine::import_pob_code(trimmed).map_err(|e| e.to_string())
                    }
                });
            match parsed {
                Ok(comp_char) => {
                    let output = pob_engine::compute_full(
                        &comp_char,
                        &app.tree,
                        Some(&app.skills),
                        app.bases.as_ref(),
                    );
                    let label = compare_tab::label_for(&comp_char);
                    app.compare_state.snapshot = Some(compare_tab::Snapshot {
                        character: comp_char,
                        output,
                        label: format!("{} (from {})", label, path.display()),
                    });
                    app.status_message = Some((
                        StatusKind::Info,
                        format!("Compare snapshot loaded from {}", path.display()),
                    ));
                }
                Err(e) => {
                    app.status_message =
                        Some((StatusKind::Error, format!("Compare load failed: {e}")));
                }
            }
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn handle_compare_action(_app: &mut LoadedApp, _action: compare_tab::CompareAction) {
    // The web build doesn't have a file picker yet; the Compare tab's
    // in-memory snapshot button still works there.
}

/// Handle a Builds-tab action: load / save / open-folder against the
/// platform-specific builds directory.
#[cfg(not(target_arch = "wasm32"))]
fn handle_builds_action(app: &mut LoadedApp, action: builds_tab::BuildsAction) {
    use builds_tab::{BuildId, BuildsAction};

    let dir = build_store_disk::builds_dir();

    match action {
        BuildsAction::Refresh => {
            if let Some(d) = &dir {
                app.builds_state.entries = build_store_disk::rescan(d);
                app.builds_state.folder_caption = format!("Folder: {}", d.display());
            } else {
                app.builds_state.entries.clear();
                app.builds_state.folder_caption =
                    "Couldn't resolve a builds directory for this platform.".into();
            }
            // Folder mode is wasm-only; never advertised on desktop.
            app.builds_state.folder_supported = false;
            app.builds_state.folder_connected = false;
        }
        BuildsAction::Load(BuildId::Disk(path)) => {
            let load = std::fs::read_to_string(&path).map_err(|e| e.to_string());
            let parsed: Result<Character, String> = load.and_then(|s| {
                let trimmed = s.trim();
                if trimmed.starts_with("MK2|") {
                    pob_engine::import_code(trimmed).map_err(|e| e.to_string())
                } else if trimmed.starts_with('<') {
                    pob_engine::import_pob_xml(trimmed).map_err(|e| e.to_string())
                } else {
                    pob_engine::import_pob_code(trimmed).map_err(|e| e.to_string())
                }
            });
            match parsed {
                Ok(c) => {
                    app.character = c;
                    app.current_build_path = Some(path.clone());
                    app.pending_seed_saved_hash = true;
                    app.dirty_since = None;
                    app.status_message =
                        Some((StatusKind::Info, format!("Opened {}", path.display())));
                }
                Err(e) => {
                    app.status_message = Some((StatusKind::Error, format!("Load failed: {e}")));
                }
            }
        }
        BuildsAction::Save { name, category } => {
            let Some(dir) = dir else {
                app.status_message = Some((
                    StatusKind::Error,
                    "Couldn't resolve a builds directory.".into(),
                ));
                return;
            };
            let mut path = dir;
            if let Some(cat) = &category {
                path.push(cat);
                let _ = std::fs::create_dir_all(&path);
            }
            path.push(format!("{name}.mk2"));
            let payload = pob_engine::export_code(&app.character).map_err(|e| e.to_string());
            match payload.and_then(|code| std::fs::write(&path, code).map_err(|e| e.to_string())) {
                Ok(()) => {
                    app.current_build_path = Some(path.clone());
                    app.last_saved_hash = app.last_compute_hash;
                    app.dirty_since = None;
                    app.status_message =
                        Some((StatusKind::Info, format!("Saved to {}", path.display())));
                }
                Err(e) => {
                    app.status_message = Some((StatusKind::Error, format!("Save failed: {e}")));
                }
            }
        }
        BuildsAction::OpenFolder => {
            let Some(dir) = dir else {
                return;
            };
            let result = if cfg!(target_os = "macos") {
                std::process::Command::new("open").arg(&dir).spawn()
            } else if cfg!(target_os = "linux") {
                std::process::Command::new("xdg-open").arg(&dir).spawn()
            } else if cfg!(target_os = "windows") {
                std::process::Command::new("explorer").arg(&dir).spawn()
            } else {
                Err(std::io::Error::other("unsupported platform"))
            };
            if let Err(e) = result {
                app.status_message = Some((
                    StatusKind::Error,
                    format!("Couldn't open {}: {e}", dir.display()),
                ));
            }
        }
        BuildsAction::Rename {
            id: BuildId::Disk(from),
            new_label,
        } => {
            let parent = from.parent().map(std::path::Path::to_path_buf);
            let ext = from
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("mk2")
                .to_owned();
            let Some(parent) = parent else {
                app.status_message = Some((
                    StatusKind::Error,
                    "Refusing to rename: no parent dir".into(),
                ));
                return;
            };
            let to = parent.join(format!("{new_label}.{ext}"));
            match std::fs::rename(&from, &to) {
                Ok(()) => {
                    if app.current_build_path.as_ref() == Some(&from) {
                        app.current_build_path = Some(to.clone());
                    }
                    app.status_message =
                        Some((StatusKind::Info, format!("Renamed to {}", to.display())));
                }
                Err(e) => {
                    app.status_message = Some((StatusKind::Error, format!("Rename failed: {e}")));
                }
            }
        }
        BuildsAction::Duplicate(BuildId::Disk(from)) => {
            // Resolve duplicate-target by looking up the entry in
            // `state.entries` so we share the helper that picks a
            // non-clashing `<name> copy.<ext>` slot.
            let entry = app
                .builds_state
                .entries
                .iter()
                .find(|e| matches!(&e.id, BuildId::Disk(p) if p == &from));
            let Some(entry) = entry else {
                app.status_message = Some((
                    StatusKind::Error,
                    "Duplicate failed: source not found".into(),
                ));
                return;
            };
            let Some(to) = build_store_disk::duplicate_target(entry) else {
                app.status_message = Some((
                    StatusKind::Error,
                    "Duplicate failed: no free copy slot".into(),
                ));
                return;
            };
            match std::fs::copy(&from, &to) {
                Ok(_) => {
                    app.status_message =
                        Some((StatusKind::Info, format!("Duplicated to {}", to.display())));
                }
                Err(e) => {
                    app.status_message =
                        Some((StatusKind::Error, format!("Duplicate failed: {e}")));
                }
            }
        }
        BuildsAction::Delete(BuildId::Disk(path)) => match std::fs::remove_file(&path) {
            Ok(()) => {
                if app.current_build_path.as_ref() == Some(&path) {
                    app.current_build_path = None;
                }
                app.status_message =
                    Some((StatusKind::Info, format!("Deleted {}", path.display())));
            }
            Err(e) => {
                app.status_message = Some((StatusKind::Error, format!("Delete failed: {e}")));
            }
        },
        BuildsAction::CreateCategory(name) => {
            let Some(dir) = dir else {
                return;
            };
            let target = dir.join(&name);
            match std::fs::create_dir_all(&target) {
                Ok(()) => {
                    app.status_message = Some((
                        StatusKind::Info,
                        format!("Created category {}", target.display()),
                    ));
                }
                Err(e) => {
                    app.status_message =
                        Some((StatusKind::Error, format!("Create category failed: {e}")));
                }
            }
        }
        // Wasm-only handle types or unreachable variants on desktop.
        BuildsAction::Load(_)
        | BuildsAction::Rename { .. }
        | BuildsAction::Duplicate(_)
        | BuildsAction::Delete(_)
        | BuildsAction::ImportFile
        | BuildsAction::ConnectFolder
        | BuildsAction::DisconnectFolder => {}
    }
}

#[cfg(target_arch = "wasm32")]
fn handle_builds_action(app: &mut LoadedApp, action: builds_tab::BuildsAction) {
    app.wasm_storage.handle_action(action, &app.character);
}

/// Wasm-only: pump completed async storage ops into the live UI state
/// at the start of each frame. Returns `true` if a [`StorageEvent::Loaded`]
/// landed (i.e. the host imported a new character and the caller
/// should force a recompute).
#[cfg(target_arch = "wasm32")]
fn apply_storage_events(app: &mut LoadedApp) -> bool {
    use build_store_wasm::StorageEvent;
    let events = app.wasm_storage.drain_events();
    if events.is_empty() {
        return false;
    }
    let mut character_changed = false;
    for event in events {
        match event {
            StorageEvent::Refreshed(entries) => {
                app.builds_state.entries = entries;
                app.builds_state.loaded = true;
            }
            StorageEvent::Loaded { label, payload } => {
                let trimmed = payload.trim();
                let parsed: Result<Character, String> = if trimmed.starts_with("MK2|") {
                    pob_engine::import_code(trimmed).map_err(|e| e.to_string())
                } else if trimmed.starts_with('<') {
                    pob_engine::import_pob_xml(trimmed).map_err(|e| e.to_string())
                } else {
                    pob_engine::import_pob_code(trimmed).map_err(|e| e.to_string())
                };
                match parsed {
                    Ok(c) => {
                        app.character = c;
                        app.pending_seed_saved_hash = true;
                        app.status_message =
                            Some((StatusKind::Info, format!("Opened \"{label}\"")));
                        character_changed = true;
                    }
                    Err(e) => {
                        app.status_message = Some((StatusKind::Error, format!("Load failed: {e}")));
                    }
                }
            }
            StorageEvent::Status(kind, msg) => {
                app.status_message = Some((kind, msg));
            }
            StorageEvent::FolderState { connected, name } => {
                app.builds_state.folder_connected = connected;
                app.builds_state.folder_caption = if connected {
                    match name {
                        Some(n) => format!("Folder: {n} (read/write via your filesystem)"),
                        None => "Folder connected".to_owned(),
                    }
                } else {
                    "Browser storage (IndexedDB) — Save also downloads the file.".to_owned()
                };
            }
            StorageEvent::FolderSupport(supported) => {
                app.builds_state.folder_supported = supported;
                if app.builds_state.folder_caption.is_empty() {
                    app.builds_state.folder_caption =
                        "Browser storage (IndexedDB) — Save also downloads the file.".to_owned();
                }
            }
        }
    }
    character_changed
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
            app.last_saved_hash = app.last_compute_hash;
            app.dirty_since = None;
            app.status_message = Some((StatusKind::Info, format!("Saved to {}", path.display())));
        }
        Err(e) => {
            app.status_message = Some((StatusKind::Error, format!("Save failed: {e}")));
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn save_build(app: &mut LoadedApp, _force_dialog: bool) {
    // Web build: route File → Save through the Builds tab's storage
    // shim. Picks a filename based on class+ascendancy so the
    // resulting download has a sensible default; users can rename
    // afterwards from the Builds tab.
    let name = default_save_name(&app.character);
    app.wasm_storage.handle_action(
        builds_tab::BuildsAction::Save {
            name,
            category: None,
        },
        &app.character,
    );
}

#[cfg(target_arch = "wasm32")]
fn default_save_name(character: &Character) -> String {
    let class = character.class.0.replace(' ', "_");
    if let Some(asc) = &character.ascendancy {
        format!("{class}_{asc}_L{lvl}", lvl = character.level)
    } else {
        format!("{class}_L{lvl}", lvl = character.level)
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
