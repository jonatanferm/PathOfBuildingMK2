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
mod cluster_overlay;
mod cluster_paste;
mod color_codes;
mod compare_tab;
mod config_tab;
#[cfg(not(target_arch = "wasm32"))]
mod ggg_fetch;
mod import_export_tab;
mod items_tab;
mod mastery_picker;
mod notes_tab;
mod party_tab;
#[cfg(not(target_arch = "wasm32"))]
mod share_url_fetch;
mod skills_tab;
mod tattoo_picker;
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
    /// Issue #205: tree-tab search QoL. `search_match_order` is the
    /// `search_matches` set in a stable, sorted order so the "Focus
    /// next match" cycler lands on a different node each time. The
    /// index advances modulo the order's length on each Enter press.
    /// `focus_search_request` is a one-shot flag set by the Cmd-F /
    /// Ctrl-F shortcut so the next frame can `request_focus()` the
    /// search text box.
    search_match_order: Vec<NodeId>,
    search_focus_index: usize,
    focus_search_request: bool,
    /// Issue #225: keyboard-shortcuts help dialog visibility. Toggled
    /// by the Help → "Keyboard shortcuts" menu item; the dialog
    /// renders an anchored egui window listing every shortcut the
    /// app recognises, mirroring PoB's inline `Ctrl+C copy` /
    /// `Ctrl+Click toggle` cheatsheets.
    show_hotkey_help: bool,
    /// Issue #220: pending tree-tab destructive-action confirmation. `Some`
    /// renders a modal asking the user to confirm. `None` means no
    /// dialog is open. Mirrors PoB's `TreeTab.lua:129-150` two-button
    /// modal popup for Reset Tree / Remove all Tattoos.
    pending_tree_reset: Option<TreeResetKind>,
    active_tab: Tab,
    items_state: items_tab::ItemsTabState,
    skills_state: skills_tab::SkillsTabState,
    calcs_state: calcs_tab::CalcsTabState,
    compare_state: compare_tab::CompareTabState,
    party_state: party_tab::PartyTabState,
    builds_state: builds_tab::BuildsTabState,
    import_export_state: import_export_tab::ImportExportTabState,
    notes_state: notes_tab::NotesTabState,
    /// Issue #98 (slice 2): right-click tattoo picker state.
    tattoo_picker_state: tattoo_picker::TattooPickerState,
    /// Issue #210: click-to-pick mastery effect dialog state.
    mastery_picker_state: mastery_picker::MasteryPickerState,
    /// Issue #197 (slice A): right-click "Paste cluster jewel" picker state.
    /// Owns the modal that lets the user paste a Cluster Jewel into a Large
    /// jewel socket on the Tree tab; the resulting `Item` is stored in
    /// `Character::jewels[socket_id]` and picked up by
    /// `compute_full_with_clusters` on the next compute pass.
    cluster_paste_state: cluster_paste::ClusterPasteState,
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
    /// Issue #21: cluster jewel category catalogue. Consumed by the
    /// sub-graph synthesis pass during compute when a Cluster Jewel is
    /// socketed; also surfaced to the Tree tab so synthesised notables
    /// can be highlighted near their host socket.
    cluster_jewels: Option<pob_data::ClusterJewelData>,
    /// Issue #21: cluster jewel notable / corrupted mods. Threaded into
    /// the engine alongside `cluster_jewels` to construct the cluster
    /// context that drives sub-graph synthesis. The data file is also
    /// reserved for a future corrupt-implicit handling slice that
    /// projects rolled-corrupt mods without re-plumbing.
    cluster_jewel_mods: Option<pob_data::ClusterModSet>,
    /// Issue #98 (slice 1+2): tattoo catalogue, surfaced by the Tree-tab right-click
    /// picker (`tattoo_picker.rs`).
    tattoos: Option<pob_data::TattooSet>,
    /// Issue #20 (slice 1+3): per-minion-type base stats. Surfaced alongside the
    /// player's output via `pob_engine::apply_minion_outputs` when the active main
    /// skill summons a minion.
    minions: Option<pob_data::MinionData>,
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

/// Issue #220: which destructive tree-tab action the user has armed but
/// not yet confirmed. Drives the confirmation modal on the Tree tab.
/// PoB shows distinct "Reset Tree" / "Remove all Tattoos" prompts —
/// mirroring with two variants keeps the dialog text accurate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TreeResetKind {
    /// Drop every allocated passive node.
    Allocation,
    /// Drop every tattoo override.
    Tattoos,
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
        let cluster_jewels = std::fs::read_to_string(data_root.join("cluster_jewels.json"))
            .ok()
            .and_then(|json| pob_data::load_cluster_jewels(&json).ok());
        let cluster_jewel_mods = std::fs::read_to_string(data_root.join("cluster_jewel_mods.json"))
            .ok()
            .and_then(|json| pob_data::load_cluster_jewel_mods(&json).ok());
        let tattoos = std::fs::read_to_string(data_root.join("tattoos.json"))
            .ok()
            .and_then(|json| pob_data::load_tattoos(&json).ok());
        let minions = std::fs::read_to_string(data_root.join("minions.json"))
            .ok()
            .and_then(|json| pob_data::load_minions(&json).ok());
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
            search_match_order: Vec::new(),
            search_focus_index: 0,
            focus_search_request: false,
            show_hotkey_help: false,
            pending_tree_reset: None,
            active_tab: Tab::Tree,
            items_state: items_tab::ItemsTabState::default(),
            skills_state: skills_tab::SkillsTabState::default(),
            calcs_state: calcs_tab::CalcsTabState::default(),
            compare_state: compare_tab::CompareTabState::default(),
            party_state: party_tab::PartyTabState::default(),
            builds_state: builds_tab::BuildsTabState::default(),
            import_export_state: import_export_tab::ImportExportTabState::default(),
            notes_state: notes_tab::NotesTabState::default(),
            tattoo_picker_state: tattoo_picker::TattooPickerState::default(),
            mastery_picker_state: mastery_picker::MasteryPickerState::default(),
            cluster_paste_state: cluster_paste::ClusterPasteState::default(),
            skills,
            bases,
            sprites,
            calc_sections,
            cluster_jewels,
            cluster_jewel_mods,
            tattoos,
            minions,
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
            search_match_order: Vec::new(),
            search_focus_index: 0,
            focus_search_request: false,
            show_hotkey_help: false,
            pending_tree_reset: None,
            active_tab: Tab::Tree,
            items_state: items_tab::ItemsTabState::default(),
            skills_state: skills_tab::SkillsTabState::default(),
            calcs_state: calcs_tab::CalcsTabState::default(),
            compare_state: compare_tab::CompareTabState::default(),
            party_state: party_tab::PartyTabState::default(),
            builds_state: builds_tab::BuildsTabState::default(),
            import_export_state: import_export_tab::ImportExportTabState::default(),
            notes_state: notes_tab::NotesTabState::default(),
            tattoo_picker_state: tattoo_picker::TattooPickerState::default(),
            mastery_picker_state: mastery_picker::MasteryPickerState::default(),
            cluster_paste_state: cluster_paste::ClusterPasteState::default(),
            skills,
            bases,
            sprites,
            calc_sections,
            // wasm doesn't bundle these yet — when a feature picks one up it can
            // decide whether to ship the JSON via include_str! or fetch it lazily.
            cluster_jewels: None,
            cluster_jewel_mods: None,
            tattoos: None,
            minions: None,
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
    let ascendancy = std::fs::read(dir.join("ascendancy.png")).ok()?;
    decode_atlas_inputs(&active, &inactive, &group, &frame, &mastery, &ascendancy)
}

#[cfg(target_arch = "wasm32")]
fn load_atlases() -> Option<tree_renderer::AtlasInputs> {
    let active = include_bytes!("../../../data/sprites/3_25/skills.jpg");
    let inactive = include_bytes!("../../../data/sprites/3_25/skills-disabled.jpg");
    let group = include_bytes!("../../../data/sprites/3_25/group-background.png");
    let frame = include_bytes!("../../../data/sprites/3_25/frame.png");
    let mastery = include_bytes!("../../../data/sprites/3_25/mastery.png");
    // Issue #110: bundle the ascendancy sprite atlas so the wasm
    // build can show the per-ascendancy medallion at each
    // AscendancyStart node.
    let ascendancy = include_bytes!("../../../data/sprites/3_25/ascendancy.png");
    decode_atlas_inputs(active, inactive, group, frame, mastery, ascendancy)
}

fn decode_atlas_inputs(
    active: &[u8],
    inactive: &[u8],
    group: &[u8],
    frame: &[u8],
    mastery: &[u8],
    ascendancy: &[u8],
) -> Option<tree_renderer::AtlasInputs> {
    let active_img = image::load_from_memory(active).ok()?.to_rgba8();
    let inactive_img = image::load_from_memory(inactive).ok()?.to_rgba8();
    let group_img = image::load_from_memory(group).ok()?.to_rgba8();
    let frame_img = image::load_from_memory(frame).ok()?.to_rgba8();
    let mastery_img = image::load_from_memory(mastery).ok()?.to_rgba8();
    let ascendancy_img = image::load_from_memory(ascendancy).ok()?.to_rgba8();
    let active_size = (active_img.width(), active_img.height());
    let inactive_size = (inactive_img.width(), inactive_img.height());
    let group_size = (group_img.width(), group_img.height());
    let frame_size = (frame_img.width(), frame_img.height());
    let mastery_size = (mastery_img.width(), mastery_img.height());
    let ascendancy_size = (ascendancy_img.width(), ascendancy_img.height());
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
        ascendancy_rgba8: ascendancy_img.into_raw(),
        ascendancy_size,
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
    // Issue #205: tree-tab search keyboard shortcuts. Cmd/Ctrl-F jumps to
    // the Tree tab and focuses the search box; Esc clears the box (and
    // matches) when the search has content. Captured at the app level so
    // they fire regardless of which panel has focus, including from
    // inside the search input itself (egui's TextEdit doesn't swallow
    // Esc by default).
    let mut focus_tree_search = false;
    let mut clear_tree_search = false;
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
        } else if cmd && i.key_pressed(egui::Key::F) {
            focus_tree_search = true;
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
        // Esc clears the search only when there's something to clear and
        // we're on the Tree tab. Without the tab gate the shortcut would
        // surprise users typing in unrelated panels (Items / Skills /
        // Notes), and without the non-empty gate it'd swallow Esc presses
        // intended for popups / modals.
        if i.key_pressed(egui::Key::Escape) && app.active_tab == Tab::Tree && !app.search.is_empty()
        {
            clear_tree_search = true;
        }
    });
    if focus_tree_search {
        app.active_tab = Tab::Tree;
        app.focus_search_request = true;
    }
    if clear_tree_search {
        app.search.clear();
        update_search(app);
    }
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
            // Issue #225: Help menu surfaces the keyboard-shortcuts
            // cheatsheet PoB scatters inline across each tab. One
            // discoverable entry point matches modern desktop-app
            // convention better than tab-local hint strings.
            ui.menu_button("Help", |ui| {
                if ui.button("Keyboard shortcuts…").clicked() {
                    app.show_hotkey_help = true;
                    ui.close_menu();
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
            // Issue #225: build-pane quick-action buttons. PoB shows
            // Save / Save As / Export at the top of the window —
            // discoverable to users who don't know the keyboard
            // shortcuts and don't want to drill into the File menu.
            // The buttons route through the same `MenuAction` path as
            // their menu-item counterparts so the behaviour stays in
            // sync. Right-aligned via the inverted layout so they
            // hug the trailing edge of the menu bar without colliding
            // with the dirty indicator.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .button("Export…")
                    .on_hover_text("Jump to the Import / Export tab to copy build XML.")
                    .clicked()
                {
                    app.active_tab = Tab::ImportExport;
                }
                if ui
                    .button("Save As…")
                    .on_hover_text("Save the build to a new file (Cmd/Ctrl+Shift+S).")
                    .clicked()
                {
                    apply_menu_action(app, MenuAction::SaveAs);
                }
                // Save is gated on having unsaved changes (or no
                // path at all) — on native; wasm doesn't track
                // dirty state, so the button stays enabled there
                // and lets the user kick off an IndexedDB write
                // unconditionally.
                #[cfg(not(target_arch = "wasm32"))]
                let save_enabled = app.dirty_since.is_some() || app.current_build_path.is_none();
                #[cfg(target_arch = "wasm32")]
                let save_enabled = true;
                if ui
                    .add_enabled(save_enabled, egui::Button::new("Save"))
                    .on_hover_text(
                        "Save the build (Cmd/Ctrl+S). Disabled \
                             when nothing has changed since the last \
                             save.",
                    )
                    .clicked()
                {
                    apply_menu_action(app, MenuAction::Save);
                }
            });
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
                let edit = egui::TextEdit::singleline(&mut app.search)
                    .desired_width(220.0)
                    .hint_text("notable name, keyword, stat...");
                let resp = ui.add(edit);
                // Issue #205: Cmd-F / Ctrl-F focuses the search box. The
                // shortcut is detected at the app-level keyboard handler
                // earlier in this frame; it sets `focus_search_request`
                // and we consume it here so the next user keystroke
                // lands in the text input.
                if app.focus_search_request {
                    resp.request_focus();
                    app.focus_search_request = false;
                }
                if resp.changed() {
                    update_search(app);
                }
                // Issue #205: Enter cycles through matches when the
                // input has focus. egui doesn't surface a "lost focus
                // because of Enter" event distinctly from a blur
                // click, so we read the raw key the frame the input
                // is focused. Wrapping `tree_view.focus` recentres the
                // viewport on each successive match so the user can
                // walk the tree without leaving the keyboard.
                if resp.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    focus_next_search_match(app);
                }
                if ui.button("Clear").clicked() {
                    app.search.clear();
                    update_search(app);
                }
                ui.separator();
                ui.label(format!("{} matches", app.tree_view.search_matches.len()));
                if ui.button("Focus next match").clicked() {
                    focus_next_search_match(app);
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
            // Issue #220: arm the destructive-action confirmation modal
            // instead of wiping allocation immediately. PoB requires a
            // confirmation click for both Reset Tree and Remove all
            // Tattoos because misclicks here lose hours of work.
            ui.horizontal(|ui| {
                if ui
                    .button("Reset allocation")
                    .on_hover_text(
                        "Drop every allocated passive node. Asks for \
                         confirmation before applying.",
                    )
                    .clicked()
                {
                    app.pending_tree_reset = Some(TreeResetKind::Allocation);
                }
                if !app.character.tattoo_overrides.is_empty()
                    && ui
                        .button("Remove all tattoos")
                        .on_hover_text(
                            "Clear every tattoo override on the tree. Asks \
                             for confirmation before applying.",
                        )
                        .clicked()
                {
                    app.pending_tree_reset = Some(TreeResetKind::Tattoos);
                }
            });
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

                // Issue #20 (slices 3-10): summoner builds want the headline
                // minion numbers next to the player's main-skill block. The
                // pipeline emits MinionLife > 0 only when the active skill
                // resolves to a minion in the catalogue, so this gate cleanly
                // suppresses the section for non-summoners.
                if app.output.get("MinionLife") > 0.0 {
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new("Minion").strong());
                    stat_row_decimal(ui, "Life", &app.output, "MinionLife");
                    // Issue #20 (slice 13): show ES only when the minion has
                    // any — most summons (Zombies, Skeletons, golems other
                    // than Carrion / Stone) emit zero, and a row of "0" adds
                    // noise without information.
                    if app.output.get("MinionEnergyShield") > 0.0 {
                        stat_row_decimal(ui, "ES", &app.output, "MinionEnergyShield");
                        // Issue #20 (slice 16): combined Life + ES pool —
                        // only meaningful when ES is non-zero, otherwise it
                        // duplicates the Life row.
                        stat_row_decimal(ui, "Total HP", &app.output, "MinionTotalHP");
                    }
                    // Issue #20 (slice 14): armour and evasion are always
                    // non-zero (the monster table values are non-zero at
                    // every level), so unlike ES they're rendered
                    // unconditionally.
                    stat_row_decimal(ui, "Armour", &app.output, "MinionArmour");
                    stat_row_decimal(ui, "Evasion", &app.output, "MinionEvasion");
                    // Issue #20 (slice 15): movement speed as a percentage
                    // (`MinionMovementSpeed`); always non-zero (baseline 100%).
                    stat_row_decimal(ui, "Move speed %", &app.output, "MinionMovementSpeed");
                    stat_row_decimal(ui, "Avg hit", &app.output, "MinionAverageDamage");
                    stat_row_decimal(ui, "Speed (cps)", &app.output, "MinionAttacksPerSecond");
                    stat_row_decimal(ui, "Crit chance %", &app.output, "MinionCritChance");
                    stat_row_decimal(ui, "Hit chance %", &app.output, "MinionHitChance");
                    stat_row_decimal(ui, "DPS", &app.output, "MinionDPS");
                    if app.output.get("MinionLifeRegen") > 0.0 {
                        stat_row_decimal(ui, "Life regen", &app.output, "MinionLifeRegen");
                    }
                    stat_row_decimal(ui, "Fire res %", &app.output, "MinionFireResist");
                    stat_row_decimal(ui, "Cold res %", &app.output, "MinionColdResist");
                    stat_row_decimal(ui, "Lightning res %", &app.output, "MinionLightningResist");
                    stat_row_decimal(ui, "Chaos res %", &app.output, "MinionChaosResist");
                }
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

    // Issue #225: keyboard-shortcuts help dialog. Renders above the
    // central panel via egui's natural z-ordering; gated on
    // `show_hotkey_help` so it only appears when the user toggled it
    // through the Help menu.
    if app.show_hotkey_help {
        render_hotkey_help_modal(ctx, &mut app.show_hotkey_help);
    }

    // Issue #220: tree-tab destructive-action confirmation modal.
    // Renders above the central panel via egui's natural z-ordering;
    // gated on `pending_tree_reset` so it appears only when the user
    // armed an action.
    if app.pending_tree_reset.is_some() {
        if let Some(applied) = render_tree_reset_modal(ctx, app) {
            // Apply the user's choice and trigger a recompute so the
            // side-panel stats refresh to the new state.
            if applied {
                recompute = true;
            }
        }
    }

    egui::CentralPanel::default().show(ctx, |ui| match app.active_tab {
        Tab::Tree => {
            let allocated: HashSet<NodeId> = app.character.allocated.iter().copied().collect();
            // Issue #98 (slice 3): tattoo badge overlay. Read the tattooed-node set
            // straight from `Character::tattoo_overrides` (engine source of truth) and
            // pass it into the renderer so it can paint a gold accent ring on each one.
            let tattooed: HashSet<NodeId> =
                app.character.tattoo_overrides.keys().copied().collect();
            // Issue #197 (slice B): hand the tree view the cluster jewel
            // overlay inputs when both data files are loaded — the renderer
            // computes synth specs and draws them around their host sockets.
            let overlay_inputs = match (&app.cluster_jewels, &app.cluster_jewel_mods) {
                (Some(cj), Some(cm)) => Some(cluster_overlay::OverlayInputs {
                    character: &app.character,
                    cluster_jewels: cj,
                    cluster_jewel_mods: cm,
                    // Tree-space layout radius around the host socket. The
                    // host's own world radius is ~75 (see
                    // `tree_view::world_radius_for(JewelSocket)`); 220 puts
                    // the synth ring well outside it without colliding with
                    // adjacent tree clusters.
                    synth_radius_world: 220.0,
                    synth_world_size: 35.0,
                }),
                _ => None,
            };
            let interaction = app.tree_view.ui_with_overlay(
                ui,
                &app.tree,
                &allocated,
                &tattooed,
                overlay_inputs.as_ref(),
            );

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
                // Issue #210: clicking an allocated mastery node opens the
                // effect picker instead of unallocating. Mastery nodes need a
                // selection to contribute stats, so toggling them off via
                // primary-click would surprise the user. Unallocation still
                // works through the path-aware unallocate flow on neighbours
                // (the engine drops mastery selections when the node leaves
                // the connected sub-graph).
                let is_mastery = node
                    .map(|n| matches!(n.kind, pob_data::NodeKind::Mastery))
                    .unwrap_or(false);

                if !allowed_ascend {
                    app.status_message = Some((
                        StatusKind::Error,
                        "Node belongs to a different ascendancy class.".into(),
                    ));
                } else if toggling_off && is_mastery {
                    app.mastery_picker_state.open_for(id);
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

            // Issue #197 (slice B): clicks on a synth (overlay) node toggle
            // its allocation directly. The synth node sits on the cluster
            // jewel sub-graph, which only contributes mods when the host
            // socket is allocated (see `apply_cluster_jewel_mods` in the
            // engine), so we gate allocation on `host_socket ∈ allocated`.
            // We bypass the regular pathfinder because synth NodeIds aren't
            // in `tree.nodes` and BFS over them would walk the synth-only
            // edges that PoB processes inline during compute.
            if let Some(synth_id) = interaction.synth_clicked {
                if app.character.allocated.contains(&synth_id) {
                    app.character.allocated.remove(&synth_id);
                    recompute = true;
                } else {
                    // Determine the host socket by re-deriving it from the
                    // synth id's bit layout — bits 6-8 carry the parent
                    // socket index. Easier: walk the current jewels to find
                    // which socket owns this id by recomputing the spec.
                    if let (Some(cj), Some(cm)) =
                        (app.cluster_jewels.as_ref(), app.cluster_jewel_mods.as_ref())
                    {
                        let specs = pob_engine::cluster_synth::synthesise_all(
                            &app.tree,
                            &app.character.jewels,
                            cj,
                            cm,
                        );
                        let host = specs
                            .iter()
                            .find(|s| s.nodes.contains_key(&synth_id))
                            .map(|s| s.parent_socket);
                        if let Some(host) = host {
                            if app.character.allocated.contains(&host) {
                                app.character.allocated.insert(synth_id);
                                recompute = true;
                            } else {
                                app.status_message = Some((
                                    StatusKind::Error,
                                    "Allocate the host jewel socket first to reach this cluster node."
                                        .into(),
                                ));
                            }
                        }
                    }
                }
            }

            // Issue #98 (slice 2) + #197 (slice A): right-click dispatches to
            // either the tattoo picker (allocated normal/notable/keystone) or
            // the cluster-jewel paste picker (Large jewel socket — has a
            // cluster jewel hosted in `expansion_jewel_size == 2`). Mastery
            // and unallocated non-socket nodes are no-ops.
            if let Some(id) = interaction.right_clicked {
                if let Some(node) = app.tree.nodes.get(&id) {
                    use pob_data::NodeKind;
                    if matches!(node.kind, NodeKind::JewelSocket)
                        && node.expansion_jewel_size == Some(2)
                    {
                        // Cluster paste flow doesn't require the socket to
                        // be allocated — the user pastes the jewel first,
                        // then walks the tree to allocate the socket and
                        // its synthesised nodes. Matches PoB's UX.
                        app.cluster_paste_state.open_for(id);
                    } else if app.character.allocated.contains(&id)
                        && matches!(node.kind, NodeKind::Mastery)
                    {
                        // Issue #210: right-click reverts (clears) the
                        // selected mastery effect. The node stays allocated;
                        // unallocation still goes through primary-click on
                        // a neighbour or the existing unallocate path.
                        if app.character.mastery_selections.remove(&id).is_some() {
                            recompute = true;
                        }
                    } else if app.character.allocated.contains(&id)
                        && matches!(
                            node.kind,
                            NodeKind::Normal | NodeKind::Notable | NodeKind::Keystone
                        )
                    {
                        app.tattoo_picker_state.open_for(id);
                    }
                }
            }

            // The picker window is rendered as a floating egui::Window so it can sit on
            // top of the tree canvas. Returns `true` when the user applied or removed a
            // tattoo, which forces a recompute.
            if tattoo_picker::ui(
                ui.ctx(),
                &mut app.tattoo_picker_state,
                app.tattoos.as_ref(),
                &app.tree,
                &mut app.character,
            ) {
                recompute = true;
            }
            // Issue #197 (slice A): cluster-jewel paste modal, same pattern.
            if cluster_paste::ui(
                ui.ctx(),
                &mut app.cluster_paste_state,
                &app.tree,
                &mut app.character,
            ) {
                recompute = true;
            }
            // Issue #210: mastery effect picker, opened by primary-click on an
            // allocated mastery node.
            if mastery_picker::ui(
                ui.ctx(),
                &mut app.mastery_picker_state,
                &app.tree,
                &mut app.character,
            ) {
                recompute = true;
            }
        }
        Tab::Items => {
            if items_tab::ui(
                ui,
                &mut app.items_state,
                &mut app.character,
                app.bases.as_ref(),
            ) {
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
            let flags = calcs_tab::active_skill_flags(&app.character, &app.skills);
            calcs_tab::ui(
                ui,
                &mut app.calcs_state,
                &app.output,
                app.last_env.as_ref(),
                app.calc_sections.as_deref(),
                &flags,
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
        // Issue #21: thread the cluster-jewel context through so any
        // socketed cluster jewels with allocated synth nodes flow their
        // mods into the calc. `None` data → fall back to non-cluster
        // compute (matches the engine's pre-#21 behaviour).
        let cluster_ctx = match (&app.cluster_jewels, &app.cluster_jewel_mods) {
            (Some(cj), Some(cm)) => Some(pob_engine::ClusterContext::new(cj, cm)),
            _ => None,
        };
        let (mut output, env) = pob_engine::compute_full_with_clusters(
            &app.character,
            &app.tree,
            Some(&app.skills),
            app.bases.as_ref(),
            cluster_ctx,
        );
        // Issue #20 (slice 3): if the active main skill summons a minion and the
        // catalogue is loaded, surface the minion's basic stats (Life / resists)
        // alongside the player's output. No-op for non-minion builds and missing
        // data — the helper returns false.
        if let Some(minions) = app.minions.as_ref() {
            pob_engine::apply_minion_outputs(
                &app.character,
                &app.skills,
                minions,
                &env,
                &mut output,
            );
        }
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

/// Issue #205: case-insensitive substring search over the tree. Returns every
/// node whose `name` or any line of `stats` contains the query, in
/// ascending node-id order so callers (Enter-cycle, Focus-next-match) see
/// a deterministic walk independent of the tree's `HashMap` iteration
/// order. An empty query returns an empty list. Pure; safe to unit-test.
fn compute_search_matches(query: &str, tree: &PassiveTree) -> Vec<NodeId> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (id, node) in &tree.nodes {
        let name_match = node
            .name
            .as_deref()
            .map(|s| s.to_lowercase().contains(&q))
            .unwrap_or(false);
        let stat_match = node.stats.iter().any(|s| s.to_lowercase().contains(&q));
        if name_match || stat_match {
            out.push(*id);
        }
    }
    out.sort_unstable();
    out
}

/// Issue #205: re-run the tree-tab search and refresh the renderer's
/// `search_matches` set + the stable-ordered match list the "Focus next
/// match" cycler walks. Resetting `search_focus_index` to zero keeps
/// Enter cycling consistent — without it, removing matches as the query
/// narrows could leave the index pointing past the new end.
fn update_search(app: &mut LoadedApp) {
    let order = compute_search_matches(&app.search, &app.tree);
    app.tree_view.search_matches.clear();
    for id in &order {
        app.tree_view.search_matches.insert(*id);
    }
    app.search_match_order = order;
    app.search_focus_index = 0;
}

/// Issue #205: focus the viewport on the next search match in `search_match_order`,
/// advancing `search_focus_index` so successive Enter presses cycle through every
/// match. No-op when the match list is empty. Returns `true` when a match was focused
/// so callers can suppress other Enter handlers if they want.
fn focus_next_search_match(app: &mut LoadedApp) -> bool {
    if app.search_match_order.is_empty() {
        return false;
    }
    let idx = app.search_focus_index % app.search_match_order.len();
    let id = app.search_match_order[idx];
    if let Some(p) = app.tree_view.position_of(id) {
        app.tree_view.focus(p.x, p.y);
    }
    app.search_focus_index = (idx + 1) % app.search_match_order.len();
    true
}

/// Issue #225: keyboard-shortcuts cheatsheet. Lists every shortcut the
/// app recognises in a two-column table (shortcut | action) so users
/// have a single discoverable reference instead of having to find
/// each tab's local hint strings. Mirrors PoB's
/// `SkillsTab.lua:110-118` inline cheatsheets, consolidated.
///
/// On Mac the natural modifier glyph is `⌘`; on Windows / Linux it's
/// `Ctrl`. egui's `Modifiers::command` already abstracts that for the
/// shortcut listener — for display, we render the platform-appropriate
/// label so users see what they'd actually press.
fn render_hotkey_help_modal(ctx: &egui::Context, show: &mut bool) {
    let cmd_label = if cfg!(target_os = "macos") {
        "⌘"
    } else {
        "Ctrl"
    };
    // Pairs of (shortcut, action) — kept compact so the modal fits in
    // a single column even on narrow screens. New shortcuts get added
    // in the same order as the keyboard handler in `render_loaded`.
    let rows: Vec<(String, &'static str)> = vec![
        (format!("{cmd_label}+N"), "New build"),
        (format!("{cmd_label}+O"), "Open build"),
        (format!("{cmd_label}+S"), "Save build"),
        (format!("{cmd_label}+Shift+S"), "Save build as…"),
        (format!("{cmd_label}+1"), "Switch to Tree tab"),
        (format!("{cmd_label}+2"), "Switch to Items tab"),
        (format!("{cmd_label}+3"), "Switch to Skills tab"),
        (format!("{cmd_label}+4"), "Switch to Config tab"),
        (format!("{cmd_label}+5"), "Switch to Calcs tab"),
        (format!("{cmd_label}+6"), "Switch to Notes tab"),
        (format!("{cmd_label}+7"), "Switch to Import / Export tab"),
        (format!("{cmd_label}+F"), "Focus the Tree-tab search box"),
        ("Enter".into(), "Cycle to next search match (in box)"),
        ("Esc".into(), "Clear the Tree-tab search"),
        (
            "Click (tree)".into(),
            "Allocate node + path from class start",
        ),
        (
            "Click allocated".into(),
            "Unallocate node + cascading orphans",
        ),
        ("Right-click (tree)".into(), "Open the tattoo picker"),
    ];
    egui::Window::new("Keyboard shortcuts")
        .open(show)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.set_min_width(360.0);
            egui::Grid::new("hotkey_help_grid")
                .num_columns(2)
                .striped(true)
                .show(ui, |ui| {
                    for (shortcut, action) in &rows {
                        ui.monospace(shortcut);
                        ui.label(*action);
                        ui.end_row();
                    }
                });
        });
}

/// Issue #220: render the destructive-action confirmation modal armed
/// by `LoadedApp::pending_tree_reset`. Returns:
/// - `Some(true)` when the user confirmed and the action ran (caller
///   should trigger a recompute).
/// - `Some(false)` when the user cancelled (no recompute needed).
/// - `None` when the modal stayed open this frame.
///
/// The modal clears `pending_tree_reset` on either button press so the
/// caller doesn't need to manage the lifecycle.
fn render_tree_reset_modal(ctx: &egui::Context, app: &mut LoadedApp) -> Option<bool> {
    let kind = app.pending_tree_reset?;
    let (title, body, confirm_label) = match kind {
        TreeResetKind::Allocation => (
            "Reset tree?",
            "This will drop every allocated passive node from the build. Are you sure?",
            "Reset tree",
        ),
        TreeResetKind::Tattoos => (
            "Remove all tattoos?",
            "This will clear every tattoo override on the tree. Are you sure?",
            "Remove tattoos",
        ),
    };
    let mut applied: Option<bool> = None;
    egui::Window::new(title)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.set_min_width(280.0);
            ui.label(body);
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    applied = Some(false);
                }
                if ui
                    .add(
                        egui::Button::new(confirm_label).fill(egui::Color32::from_rgb(140, 50, 50)),
                    )
                    .clicked()
                {
                    match kind {
                        TreeResetKind::Allocation => {
                            app.character.allocated.clear();
                        }
                        TreeResetKind::Tattoos => {
                            app.character.tattoo_overrides.clear();
                        }
                    }
                    applied = Some(true);
                }
            });
        });
    if applied.is_some() {
        app.pending_tree_reset = None;
    }
    applied
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
                    quality_id: pob_engine::QualityId::Default,
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

#[cfg(test)]
mod search_tests {
    use super::*;
    use ahash::HashMap as AHashMap;
    use pob_data::{Node, NodeKind, TreeConstants, TreePoints};

    fn empty_tree() -> PassiveTree {
        PassiveTree {
            version: "test".into(),
            tree: "test".into(),
            classes: vec![],
            groups: AHashMap::default(),
            nodes: AHashMap::default(),
            jewel_slots: vec![],
            min_x: 0,
            min_y: 0,
            max_x: 0,
            max_y: 0,
            constants: TreeConstants {
                skills_per_orbit: vec![],
                orbit_radii: vec![],
                classes: AHashMap::default(),
                character_attributes: AHashMap::default(),
                pss_centre_inner_radius: None,
            },
            points: TreePoints::default(),
        }
    }

    fn add_node(tree: &mut PassiveTree, id: NodeId, name: Option<&str>, stats: &[&str]) {
        tree.nodes.insert(
            id,
            Node {
                id,
                name: name.map(str::to_owned),
                icon: None,
                ascendancy_name: None,
                stats: stats.iter().map(|s| (*s).to_owned()).collect(),
                reminder_text: vec![],
                kind: NodeKind::Notable,
                class_start_index: None,
                group: None,
                orbit: None,
                orbit_index: None,
                out_edges: Default::default(),
                in_edges: Default::default(),
                mastery_effects: vec![],
                expansion_jewel_size: None,
                jewel_radius: None,
            },
        );
    }

    /// Issue #205: empty / whitespace-only queries return zero matches so
    /// the renderer's highlight ring stays off.
    #[test]
    fn empty_query_returns_no_matches() {
        let mut tree = empty_tree();
        add_node(&mut tree, 1, Some("Frenzy Charge"), &[]);
        assert!(compute_search_matches("", &tree).is_empty());
        assert!(compute_search_matches("   ", &tree).is_empty());
    }

    /// Issue #205: substring matches against `name`.
    #[test]
    fn name_substring_match() {
        let mut tree = empty_tree();
        add_node(&mut tree, 1, Some("Frenzy Adept"), &[]);
        add_node(&mut tree, 2, Some("Mana Wraith"), &[]);
        add_node(&mut tree, 3, Some("Berserker Frenzy"), &[]);
        let matches = compute_search_matches("Frenzy", &tree);
        assert_eq!(matches, vec![1, 3]);
    }

    /// Issue #205: substring matches against any `stats` line, even when
    /// the node name itself is missing or doesn't contain the keyword.
    #[test]
    fn stats_substring_match() {
        let mut tree = empty_tree();
        add_node(&mut tree, 1, Some("Generic"), &["+10 to maximum Life"]);
        add_node(
            &mut tree,
            2,
            Some("Generic"),
            &["10% increased Frenzy Charge Duration"],
        );
        add_node(&mut tree, 3, None, &["Gain a Frenzy Charge on Kill"]);
        let matches = compute_search_matches("Frenzy", &tree);
        assert_eq!(matches, vec![2, 3]);
    }

    /// Issue #205: search is case-insensitive — "frenzy" must match
    /// "Frenzy Adept" and "FRENZY" must match "frenzy charge".
    #[test]
    fn case_insensitive_match() {
        let mut tree = empty_tree();
        add_node(&mut tree, 1, Some("Frenzy Adept"), &[]);
        add_node(&mut tree, 2, None, &["frenzy charge"]);
        assert_eq!(compute_search_matches("frenzy", &tree), vec![1, 2]);
        assert_eq!(compute_search_matches("FRENZY", &tree), vec![1, 2]);
        assert_eq!(compute_search_matches("FrEnZy", &tree), vec![1, 2]);
    }

    /// Issue #205: returned ids are sorted ascending so the Enter-cycle
    /// walks the tree deterministically frame-to-frame, independent of
    /// the underlying HashMap iteration order.
    #[test]
    fn matches_returned_in_ascending_node_id_order() {
        let mut tree = empty_tree();
        // Insert in non-sorted order on purpose so HashMap iteration would
        // typically return them out-of-order.
        add_node(&mut tree, 42, Some("Frenzy Notable"), &[]);
        add_node(&mut tree, 7, Some("Frenzy Mover"), &[]);
        add_node(&mut tree, 100, Some("Frenzy Stand"), &[]);
        add_node(&mut tree, 3, Some("Frenzy First"), &[]);
        let matches = compute_search_matches("Frenzy", &tree);
        assert_eq!(matches, vec![3, 7, 42, 100]);
    }

    /// Issue #205: a node that doesn't match by name OR stats is dropped.
    /// Guards against accidentally matching against `reminder_text` or
    /// other fields that PoB doesn't search.
    #[test]
    fn unrelated_nodes_are_excluded() {
        let mut tree = empty_tree();
        add_node(&mut tree, 1, Some("Alacrity"), &["+10 Strength"]);
        add_node(&mut tree, 2, Some("Frenzy Adept"), &[]);
        let matches = compute_search_matches("Frenzy", &tree);
        assert_eq!(matches, vec![2]);
    }
}
