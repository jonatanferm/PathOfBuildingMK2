//! Builds tab — UI shell for the cross-platform build browser.
//!
//! On desktop the host backs this with the platform-specific builds
//! directory (see [`build_store_disk`]). On wasm the host backs it
//! with [`build_store_wasm`], which stores `.mk2` payloads in
//! IndexedDB and optionally mirrors a real folder via the File System
//! Access API. Either way, this module is concerned only with
//! rendering the listing + emitting [`BuildsAction`]s; it never
//! touches storage directly.

use std::collections::HashMap;
use std::path::PathBuf;

use eframe::egui;

use crate::builds_folder_ctx_menu::{
    can_commit, start_delete_folder, start_new_subfolder, start_rename_folder, validation_error,
    DeleteFolderError, FolderPopupState,
};
use crate::builds_folder_ops::validate_folder_name;
use crate::builds_folder_tree::{build_folder_tree, folder_path_key, FolderNode};

/// Opaque handle for a saved build. Concrete shape depends on the
/// active storage backend; the UI treats it as an equality-checkable
/// token. Variants are cross-platform — the unused ones are dead-code
/// per cfg, but suppressing the warning keeps both source-tree and
/// downstream pattern-matching uniform.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BuildId {
    /// Absolute filesystem path. Desktop only.
    Disk(PathBuf),
    /// IndexedDB record key (uuid). Wasm only.
    Idb(String),
    /// Filename (relative to the connected directory) inside a File
    /// System Access folder. Wasm only.
    Folder(String),
}

#[derive(Debug, Clone, Default)]
pub struct BuildsTabState {
    /// Most recent listing supplied by the host. Refresh requests are
    /// emitted as `BuildsAction::Refresh`; the host populates this
    /// field and bumps `loaded`.
    pub entries: Vec<BuildEntry>,
    /// Cleared by the host whenever it serves a fresh listing. The UI
    /// treats `false` as "ask host to refresh on next frame."
    pub loaded: bool,
    /// Buffer for the "save current as <name>" input.
    pub save_buffer: String,
    /// Issue #100 (slice 3): inline rename state. When `Some`, the
    /// matching build entry renders a TextEdit in place of its label
    /// until the user confirms or cancels.
    pub pending_rename: Option<(BuildId, String)>,
    /// Issue #100 (slice 3): two-step delete confirmation. The first
    /// click on Delete records the id here; the second click on the
    /// same row's now-red "Confirm?" button fires the action.
    pub pending_delete: Option<BuildId>,
    /// Issue #100 (slice 3): buffer for the "+ New category" input.
    pub new_category_buffer: String,
    /// Caption shown above the list. Desktop sets this to the resolved
    /// builds dir; wasm sets it to "Browser storage" or the connected
    /// folder name. Empty means hide the caption.
    pub folder_caption: String,
    /// Whether the host's current backend is a File System Access
    /// folder (wasm only). Drives the visibility of the
    /// connect/disconnect controls.
    pub folder_connected: bool,
    /// Whether the running browser exposes `showDirectoryPicker`. On
    /// desktop / unsupported wasm browsers this is false and the
    /// "Connect folder" button doesn't render. The host updates this
    /// at boot.
    pub folder_supported: bool,
    /// Wasm-only: whether the host is mid-write of the most recent
    /// save and we should disable Save-here briefly. Currently unused
    /// (writes happen synchronously from the UI's perspective) but
    /// reserved so a future debounce can hook in.
    pub busy: bool,
    /// Issue #213 (slice 2): per-folder expand/collapse state, keyed
    /// by [`crate::builds_folder_tree::folder_path_key`] (slash-joined
    /// path of folder names from the root). Survives tab switches and
    /// list refreshes. Missing keys default to "expanded" so a fresh
    /// listing shows everything.
    pub expanded: HashMap<String, bool>,
    /// Issue #213 (slice 4): folder right-click context-menu popup
    /// state. Carries the in-flight rename / new-subfolder / delete
    /// operation. Only one popup is open at a time — opening a
    /// second one closes the first, mirroring the build-row rename
    /// popup. See [`crate::builds_folder_ctx_menu`] for the state
    /// machine.
    pub folder_popup: Option<FolderPopupState>,
    /// Issue #213 (slice 4): transient inline status for the folder
    /// context menu — currently used to surface "Folder is not empty
    /// (N builds)" when the user picks Delete on a non-empty folder.
    /// Cleared on the next folder-popup open.
    pub folder_popup_status: Option<String>,
}

#[derive(Debug, Clone)]
pub struct BuildEntry {
    /// File-stem display label (e.g. "MyBuild").
    pub label: String,
    /// Backend handle for load/rename/delete operations.
    pub id: BuildId,
    /// File extension lowercased (`mk2` / `xml`).
    pub ext: String,
    /// Issue #100 (slice 2): subdirectory the build lives under,
    /// relative to the builds root. `None` means the file is in the
    /// root itself ("Uncategorised" group). Mirrors PoB's
    /// "Levelling/", "Bossing/", "Mapping/" folder convention.
    pub category: Option<String>,
}

/// Action the host should take based on the user's interaction.
/// Variants prefixed with "wasm only" never construct on desktop —
/// the `dead_code` allow keeps both targets sharing one match arm
/// surface in the host without per-cfg variants.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildsAction {
    /// User asked to load this build.
    Load(BuildId),
    /// User asked to save the current build under this name. The host
    /// resolves the destination (disk path / IDB record / folder file)
    /// and stores the payload.
    Save {
        name: String,
        category: Option<String>,
    },
    /// Re-list builds. Emitted on first frame and after the
    /// Refresh button.
    Refresh,
    /// User clicked "Open folder" — desktop opens the builds dir in
    /// the platform file manager. Wasm: ignored.
    OpenFolder,
    /// Rename a build to a new label (no extension).
    Rename { id: BuildId, new_label: String },
    /// Duplicate a build. Host picks a `<name> copy.<ext>`-style new
    /// name that doesn't clash.
    Duplicate(BuildId),
    /// Permanently delete a build.
    Delete(BuildId),
    /// Create an empty category (subdirectory on disk, virtual on
    /// wasm).
    CreateCategory(String),
    /// Wasm only: open a file picker, parse the chosen file, store it
    /// as a new build. No-op on desktop (use the File menu instead).
    ImportFile,
    /// Wasm only: prompt the user to grant access to a folder via the
    /// File System Access API and switch to folder-backed storage.
    ConnectFolder,
    /// Wasm only: drop the connected folder and revert to IndexedDB
    /// storage.
    DisconnectFolder,
    /// Issue #213 (slice 4): rename a folder on disk. `from_path` is
    /// the slash-joined folder path key (matches
    /// [`crate::builds_folder_tree::folder_path_key`]); `new_name` is
    /// the new leaf segment (already validated by
    /// [`crate::builds_folder_ops::validate_folder_name`]).
    RenameFolder { from_path: String, new_name: String },
    /// Issue #213 (slice 4): create an empty subfolder inside the
    /// folder identified by `parent_path`. `name` is already
    /// validated. The host calls `std::fs::create_dir`.
    CreateSubfolder { parent_path: String, name: String },
    /// Issue #213 (slice 4): delete an empty folder. The renderer
    /// only emits this for folders the empty-check passed — see
    /// [`crate::builds_folder_ctx_menu::start_delete_folder`].
    DeleteFolder { path: String },
}

/// Render the builds tab. Returns an action for the host to execute
/// (load file, save current under a name, refresh, etc.), or `None`
/// if the user only browsed.
pub fn ui(ui: &mut egui::Ui, state: &mut BuildsTabState) -> Option<BuildsAction> {
    let mut action: Option<BuildsAction> = None;

    if !state.loaded {
        // First frame after a state reset — ask the host to populate
        // the listing. We optimistically flip `loaded` so we don't
        // re-emit on every subsequent frame; the host clears it again
        // if it wants another refresh.
        state.loaded = true;
        action = Some(BuildsAction::Refresh);
    }

    ui.horizontal(|ui| {
        ui.heading("Builds");
        ui.separator();
        if ui.button("Refresh").clicked() {
            // Direct emit overrides the implicit first-frame Refresh
            // we may have already queued; the host treats them
            // identically.
            action = Some(BuildsAction::Refresh);
        }
        if ui.button("Open folder").clicked() {
            action = Some(BuildsAction::OpenFolder);
        }
        // Wasm only: show File System Access controls when the
        // browser supports them. Hidden on desktop and unsupported
        // browsers, so the desktop UI doesn't grow a useless button.
        if state.folder_supported {
            if state.folder_connected {
                if ui
                    .button("Disconnect folder")
                    .on_hover_text("Stop using the connected folder; revert to browser storage.")
                    .clicked()
                {
                    action = Some(BuildsAction::DisconnectFolder);
                }
            } else if ui
                .button("Connect folder")
                .on_hover_text(
                    "Pick a folder on your computer that the app can read and write directly.",
                )
                .clicked()
            {
                action = Some(BuildsAction::ConnectFolder);
            }
        }
        // Wasm only: explicit upload entry-point. Desktop users have
        // File → Open instead.
        #[cfg(target_arch = "wasm32")]
        if ui
            .button("Import file…")
            .on_hover_text("Pick a .mk2 / .xml file from your computer and add it to the list.")
            .clicked()
        {
            action = Some(BuildsAction::ImportFile);
        }
    });
    if !state.folder_caption.is_empty() {
        ui.weak(&state.folder_caption);
    }

    // Issue #100 (slice 3): "+ New category" creates an empty subdir
    // (or a virtual category in wasm IDB mode) so subsequent Save
    // calls can drop builds into it.
    ui.horizontal(|ui| {
        ui.label("New category:");
        ui.add(
            egui::TextEdit::singleline(&mut state.new_category_buffer)
                .desired_width(180.0)
                .hint_text("e.g. Levelling"),
        );
        let trimmed = state.new_category_buffer.trim();
        let create_enabled = !trimmed.is_empty()
            && !trimmed.contains('/')
            && !trimmed.contains('\\')
            && !trimmed.starts_with('.');
        if ui
            .add_enabled(create_enabled, egui::Button::new("+ Create"))
            .on_hover_text("Creates an empty category. Drop saves into it via Save here later.")
            .clicked()
        {
            action = Some(BuildsAction::CreateCategory(trimmed.to_owned()));
            state.new_category_buffer.clear();
            state.loaded = false;
        }
    });
    ui.separator();

    if state.entries.is_empty() {
        ui.label(
            "No builds saved yet. Save the current build into the list via \"Save here\" \
             below — or use Import file… to load one from your computer.",
        );
    } else {
        // Issue #213 (slice 2): nested folder tree. The category
        // string is treated as a `/`-separated path so categories like
        // "Levelling/Marauder" produce a nested expandable folder.
        // Builds with no category land at the root (was
        // "Uncategorised" in slice 1; now they just appear at the top
        // of the tree).
        let tree = build_folder_tree(&state.entries);
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .max_height(360.0)
            .show(ui, |ui| {
                let mut path: Vec<&str> = Vec::new();
                render_folder(ui, &tree, &mut path, state, &mut action);
            });
        // Issue #213 (slice 4): inline status from the folder
        // context menu (currently "folder not empty" on Delete).
        if let Some(msg) = &state.folder_popup_status {
            ui.colored_label(egui::Color32::from_rgb(0xDD, 0x00, 0x22), msg);
        }
        // Render any open folder popup (rename / new subfolder /
        // delete confirm). Always renders after the tree so the
        // window draws on top.
        render_folder_popup(ui, state, &mut action);
    }

    ui.separator();
    ui.horizontal(|ui| {
        ui.label("Save current build as:");
        ui.add(
            egui::TextEdit::singleline(&mut state.save_buffer)
                .desired_width(220.0)
                .hint_text("filename (no extension)"),
        );
        let trimmed = state.save_buffer.trim();
        let save_enabled = !trimmed.is_empty() && !state.busy;
        if ui
            .add_enabled(save_enabled, egui::Button::new("Save here"))
            .clicked()
        {
            action = Some(BuildsAction::Save {
                name: trimmed.to_owned(),
                category: None,
            });
            state.save_buffer.clear();
            state.loaded = false; // refresh after the host writes
        }
    });

    action
}

/// Recursively render a folder node and all of its children. The
/// root node is rendered transparently — its builds and subfolders
/// appear at the top level — and only named subfolders get a
/// `CollapsingHeader`.
///
/// `path` accumulates the chain of folder names from the root down
/// to the current node; together with [`folder_path_key`] this
/// becomes the persistent expand-state key in
/// [`BuildsTabState::expanded`] and the egui id_salt (so two folders
/// with the same leaf name at different depths don't collide).
fn render_folder<'a>(
    ui: &mut egui::Ui,
    node: &'a FolderNode,
    path: &mut Vec<&'a str>,
    state: &mut BuildsTabState,
    action: &mut Option<BuildsAction>,
) {
    // Subfolders first (PoB convention also enforced by the data
    // layer's sort).
    for child in &node.children {
        path.push(&child.name);
        let key = folder_path_key(path);
        let total = count_builds(child);
        let header = format!("{name}  ({total})", name = child.name);
        let default_open = *state.expanded.get(&key).unwrap_or(&true);
        let resp = egui::CollapsingHeader::new(header)
            .id_salt(format!("builds_folder_{key}"))
            .default_open(default_open)
            .show(ui, |ui| {
                render_folder(ui, child, path, state, action);
            });
        // Persist the (possibly toggled) open state so it survives
        // refresh / tab switches.
        state.expanded.insert(key.clone(), resp.openness > 0.5);
        // Issue #213 (slice 4): right-click context menu on the
        // folder header. The three entries seed the matching popup
        // state; the popup itself renders after the tree (see
        // `render_folder_popup`). Delete short-circuits to a status
        // message on a non-empty folder so the confirm modal doesn't
        // open just to refuse on Confirm.
        resp.header_response.context_menu(|ui| {
            if ui.button("Rename folder…").clicked() {
                state.folder_popup = Some(start_rename_folder(key.clone(), &child.name));
                state.folder_popup_status = None;
                ui.close_menu();
            }
            if ui.button("New subfolder…").clicked() {
                state.folder_popup = Some(start_new_subfolder(key.clone()));
                state.folder_popup_status = None;
                ui.close_menu();
            }
            if ui.button("Delete folder").clicked() {
                match start_delete_folder(key.clone(), child) {
                    Ok(popup) => {
                        state.folder_popup = Some(popup);
                        state.folder_popup_status = None;
                    }
                    Err(DeleteFolderError::NotEmpty { build_count }) => {
                        state.folder_popup = None;
                        state.folder_popup_status = Some(format!(
                            "Can't delete \"{}\" — folder still contains {build_count} build(s).",
                            child.name,
                        ));
                    }
                }
                ui.close_menu();
            }
        });
        path.pop();
    }
    // Then builds in this folder.
    for entry in &node.builds {
        render_build_row(ui, entry, state, action);
    }
}

/// Count the total number of builds reachable from this node
/// (including all nested subfolders) so the folder header can show
/// a useful badge without forcing the user to expand it first.
fn count_builds(node: &FolderNode) -> usize {
    let mut n = node.builds.len();
    for child in &node.children {
        n += count_builds(child);
    }
    n
}

/// Render a single build entry row (label, ext, rename / duplicate /
/// delete controls). Extracted so both the root level and every
/// nested folder share one code path.
fn render_build_row(
    ui: &mut egui::Ui,
    entry: &BuildEntry,
    state: &mut BuildsTabState,
    action: &mut Option<BuildsAction>,
) {
    ui.horizontal(|ui| {
        let renaming_this = state
            .pending_rename
            .as_ref()
            .map(|(id, _)| id == &entry.id)
            .unwrap_or(false);
        if renaming_this {
            let buf = state
                .pending_rename
                .as_mut()
                .map(|(_, s)| s)
                .expect("just checked Some");
            let resp = ui.add(
                egui::TextEdit::singleline(buf)
                    .desired_width(180.0)
                    .font(egui::TextStyle::Monospace),
            );
            let confirm = ui.button("OK").on_hover_text("Apply rename").clicked()
                || (resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
            let cancel = ui.button("✕").on_hover_text("Cancel rename").clicked();
            if confirm {
                let new_name = buf.trim().to_owned();
                if !new_name.is_empty() && new_name != entry.label {
                    *action = Some(BuildsAction::Rename {
                        id: entry.id.clone(),
                        new_label: new_name,
                    });
                    state.loaded = false;
                }
                state.pending_rename = None;
            } else if cancel {
                state.pending_rename = None;
            }
        } else if ui
            .add(
                egui::Label::new(egui::RichText::new(&entry.label).monospace())
                    .sense(egui::Sense::click()),
            )
            .on_hover_text("Click to load")
            .clicked()
        {
            *action = Some(BuildsAction::Load(entry.id.clone()));
        }

        ui.weak(format!(".{}", entry.ext));

        if !renaming_this {
            if ui.small_button("✎").on_hover_text("Rename").clicked() {
                state.pending_rename = Some((entry.id.clone(), entry.label.clone()));
                state.pending_delete = None;
            }
            if ui.small_button("⎘").on_hover_text("Duplicate").clicked() {
                *action = Some(BuildsAction::Duplicate(entry.id.clone()));
                state.loaded = false;
            }
            let pending = state
                .pending_delete
                .as_ref()
                .map(|id| id == &entry.id)
                .unwrap_or(false);
            if pending {
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new("Confirm?")
                                .color(egui::Color32::from_rgb(0xDD, 0x00, 0x22)),
                        )
                        .small(),
                    )
                    .on_hover_text("Click again to permanently delete")
                    .clicked()
                {
                    *action = Some(BuildsAction::Delete(entry.id.clone()));
                    state.pending_delete = None;
                    state.loaded = false;
                }
            } else if ui
                .small_button("🗑")
                .on_hover_text("Delete (click again to confirm)")
                .clicked()
            {
                state.pending_delete = Some(entry.id.clone());
            }
        }
    });
}

/// Issue #213 (slice 4): render the folder context-menu popup.
/// Three flavours share one window so the user only ever sees one
/// folder dialog at a time. Save commits via the matching
/// [`BuildsAction`] variant; Cancel / outside-click closes the
/// popup without emitting.
fn render_folder_popup(
    ui: &mut egui::Ui,
    state: &mut BuildsTabState,
    action: &mut Option<BuildsAction>,
) {
    let Some(popup) = state.folder_popup.as_ref() else {
        return;
    };
    let title = match popup {
        FolderPopupState::Rename { .. } => "Rename folder",
        FolderPopupState::NewSubfolder { .. } => "New subfolder",
        FolderPopupState::DeleteConfirm { .. } => "Delete folder",
    };
    let mut open = true;
    let mut close = false;
    let mut commit = false;
    egui::Window::new(title)
        .id(egui::Id::new("builds-folder-popup"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ui.ctx(), |ui| match state.folder_popup.as_mut() {
            Some(FolderPopupState::Rename {
                path,
                buffer,
                error,
            }) => {
                ui.label(format!("Rename folder: {path}"));
                let resp = ui.add(
                    egui::TextEdit::singleline(buffer)
                        .desired_width(220.0)
                        .hint_text("new folder name"),
                );
                *error = validate_folder_name(buffer).err();
                if let Some(err) = error.as_ref() {
                    ui.colored_label(egui::Color32::from_rgb(0xDD, 0x00, 0x22), err.to_string());
                }
                let popup_for_check = FolderPopupState::Rename {
                    path: path.clone(),
                    buffer: buffer.clone(),
                    error: error.clone(),
                };
                let enabled = can_commit(&popup_for_check);
                ui.horizontal(|ui| {
                    if ui.add_enabled(enabled, egui::Button::new("Save")).clicked()
                        || (enabled
                            && resp.lost_focus()
                            && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                    {
                        commit = true;
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                });
            }
            Some(FolderPopupState::NewSubfolder {
                parent_path,
                buffer,
                error,
            }) => {
                let label = if parent_path.is_empty() {
                    "Create subfolder at root".to_owned()
                } else {
                    format!("Create subfolder inside: {parent_path}")
                };
                ui.label(label);
                let resp = ui.add(
                    egui::TextEdit::singleline(buffer)
                        .desired_width(220.0)
                        .hint_text("new folder name"),
                );
                *error = validation_error(buffer);
                if let Some(err) = error.as_ref() {
                    ui.colored_label(egui::Color32::from_rgb(0xDD, 0x00, 0x22), err.to_string());
                }
                let popup_for_check = FolderPopupState::NewSubfolder {
                    parent_path: parent_path.clone(),
                    buffer: buffer.clone(),
                    error: error.clone(),
                };
                let enabled = can_commit(&popup_for_check);
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(enabled, egui::Button::new("Create"))
                        .clicked()
                        || (enabled
                            && resp.lost_focus()
                            && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                    {
                        commit = true;
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                });
            }
            Some(FolderPopupState::DeleteConfirm { path }) => {
                ui.label(format!(
                    "Permanently delete folder \"{path}\"? This removes the empty directory \
                     from disk.",
                ));
                ui.horizontal(|ui| {
                    if ui
                        .add(egui::Button::new(
                            egui::RichText::new("Confirm delete")
                                .color(egui::Color32::from_rgb(0xDD, 0x00, 0x22)),
                        ))
                        .clicked()
                    {
                        commit = true;
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                });
            }
            None => {}
        });
    if commit {
        // Build the matching action from the popup state, then close.
        match state.folder_popup.take() {
            Some(FolderPopupState::Rename { path, buffer, .. }) => {
                if let Ok(new_name) = validate_folder_name(&buffer) {
                    *action = Some(BuildsAction::RenameFolder {
                        from_path: path,
                        new_name,
                    });
                    state.loaded = false;
                }
            }
            Some(FolderPopupState::NewSubfolder {
                parent_path,
                buffer,
                ..
            }) => {
                if let Ok(name) = validate_folder_name(&buffer) {
                    *action = Some(BuildsAction::CreateSubfolder { parent_path, name });
                    state.loaded = false;
                }
            }
            Some(FolderPopupState::DeleteConfirm { path }) => {
                *action = Some(BuildsAction::DeleteFolder { path });
                state.loaded = false;
            }
            None => {}
        }
        state.folder_popup_status = None;
    } else if close || !open {
        state.folder_popup = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(label: &str, category: Option<&str>) -> BuildEntry {
        BuildEntry {
            label: label.to_owned(),
            id: BuildId::Disk(PathBuf::from(format!("/tmp/{label}.mk2"))),
            ext: "mk2".to_owned(),
            category: category.map(str::to_owned),
        }
    }

    #[test]
    fn count_builds_recurses_into_subfolders() {
        let entries = vec![
            entry("a", None),
            entry("b", Some("Lev")),
            entry("c", Some("Lev/Mar")),
            entry("d", Some("Lev/Mar")),
            entry("e", Some("Boss")),
        ];
        let tree = build_folder_tree(&entries);
        // Total reachable from the root = every entry.
        assert_eq!(count_builds(&tree), 5);
        // Lev = b + Mar/{c,d} = 3.
        let lev = tree
            .children
            .iter()
            .find(|c| c.name == "Lev")
            .expect("Lev folder");
        assert_eq!(count_builds(lev), 3);
        // Boss = just e.
        let boss = tree
            .children
            .iter()
            .find(|c| c.name == "Boss")
            .expect("Boss folder");
        assert_eq!(count_builds(boss), 1);
    }

    #[test]
    fn count_builds_empty_tree_is_zero() {
        let tree = build_folder_tree(&[]);
        assert_eq!(count_builds(&tree), 0);
    }

    #[test]
    fn expanded_state_defaults_to_open_for_unknown_keys() {
        // Documents the contract the renderer relies on: a freshly
        // populated listing (no expand state stored yet) shows every
        // folder open by default.
        let state = BuildsTabState::default();
        assert!(state.expanded.is_empty());
        let key = folder_path_key(&["Levelling"]);
        assert!(*state.expanded.get(&key).unwrap_or(&true));
    }

    #[test]
    fn expanded_state_persists_collapsed_keys() {
        // Renderer writes `false` for a collapsed folder. A
        // subsequent frame should observe that — i.e. the map is
        // round-trippable and folder_path_key is stable.
        let mut state = BuildsTabState::default();
        let key = folder_path_key(&["Levelling", "Marauder"]);
        state.expanded.insert(key.clone(), false);
        assert_eq!(state.expanded.get(&key), Some(&false));
        // Sibling folder unaffected.
        let other = folder_path_key(&["Levelling", "Witch"]);
        assert!(*state.expanded.get(&other).unwrap_or(&true));
    }
}
