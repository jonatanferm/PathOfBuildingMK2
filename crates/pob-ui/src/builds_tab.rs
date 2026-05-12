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
use crate::builds_folder_tree::{
    build_folder_tree, build_folder_tree_sorted, folder_path_key, BuildsSortMode, FolderNode,
};

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
    /// Issue #213 (slice 5): id of the build whose "Move to folder…"
    /// popup is currently open. `None` means no popup. Only one move
    /// popup at a time so the renderer doesn't have to disambiguate
    /// which build the popup belongs to.
    pub move_popup_for: Option<BuildId>,
    /// Issue #213 (folder-isolation slice): slash-joined path of the
    /// folder the user has filtered the list to via "Show only this
    /// folder". `None` means "no filter — show every folder". The
    /// renderer hides every sibling branch when this is set and
    /// surfaces a chip row offering to clear the filter.
    pub selected_folder: Option<String>,
    /// Issue #213 follow-up: which order builds appear in within each
    /// folder. Defaults to `Name` (the historical case-insensitive
    /// alphabetical order); `RecentFirst` puts the most-recently-
    /// modified builds at the top so iterating users find their
    /// just-saved files without scrolling.
    pub sort_mode: BuildsSortMode,
    /// Issue #213 follow-up: case-insensitive substring filter applied to
    /// build labels before the tree is constructed. Empty / whitespace-
    /// only is "no filter". Match is on label only — see
    /// [`crate::builds_folder_tree::filter_entries_by_name`].
    pub name_filter: String,
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
    /// Issue #213 follow-up: filesystem modification time, when the
    /// backend can supply one. Surfaced in the build-row hover as a
    /// relative "modified X ago" hint so users can spot recently-
    /// touched builds at a glance. `None` for in-memory entries
    /// (test fixtures) and wasm IDB-backed builds without an mtime.
    pub modified: Option<std::time::SystemTime>,
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
    /// Issue #213 (slice 5): move a build into a different folder.
    /// `target` is the slash-joined folder path the build should
    /// land in (`None` and `Some("")` mean the root). The host calls
    /// `std::fs::rename` after computing the destination via
    /// [`crate::build_store_disk::move_to_folder_target`].
    MoveBuild { id: BuildId, target: Option<String> },
}

/// Render the builds tab. Returns an action for the host to execute
/// (load file, save current under a name, refresh, etc.), or `None`
/// if the user only browsed.
pub fn ui(
    ui: &mut egui::Ui,
    state: &mut BuildsTabState,
    current_path: Option<&std::path::Path>,
) -> Option<BuildsAction> {
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
    // Issue #213 follow-up: sort mode selector. Mirrors PoB's
    // `Builds sort` dropdown — `Recent` puts the freshly-saved build
    // at the top of its folder, `Name` is the historical alphabetical
    // order. Only renders when there's something to sort to keep the
    // empty-state pane tidy.
    if !state.entries.is_empty() {
        ui.horizontal(|ui| {
            ui.label("Sort by:");
            for mode in [BuildsSortMode::Name, BuildsSortMode::RecentFirst] {
                ui.selectable_value(&mut state.sort_mode, mode, mode.label());
            }
        });
        // Issue #213 follow-up: case-insensitive substring filter on
        // build labels. Empty input is the cold-open default and
        // surfaces every build. Folder-level scoping stays opt-in via
        // "Show only this folder" below.
        ui.horizontal(|ui| {
            ui.label("Filter:");
            ui.add(
                egui::TextEdit::singleline(&mut state.name_filter)
                    .hint_text("build name…")
                    .desired_width(220.0),
            );
            if !state.name_filter.trim().is_empty()
                && ui
                    .small_button("✕")
                    .on_hover_text("Clear the build-name filter.")
                    .clicked()
            {
                state.name_filter.clear();
            }
        });
    }
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
        // Issue #213 follow-up: apply the name filter before tree
        // construction so empty folders (every leaf filtered out)
        // naturally drop out of the rendered tree.
        let filtered_entries =
            crate::builds_folder_tree::filter_entries_by_name(&state.entries, &state.name_filter);
        let full_tree = build_folder_tree_sorted(&filtered_entries, state.sort_mode);
        // Issue #213 (folder-isolation slice): when the user picks
        // "Show only this folder", drop every sibling branch via the
        // pure subtree-extract helper. The chip row below offers a
        // one-click clear. Falling back to the full tree when the
        // remembered path no longer resolves keeps the UI usable
        // after the user deletes the filtered folder elsewhere.
        let (tree, root_path_segments): (FolderNode, Vec<String>) =
            if let Some(key) = state.selected_folder.clone() {
                if let Some(subtree) =
                    crate::builds_folder_tree::filter_folder_to_subtree(&full_tree, &key)
                {
                    let segs: Vec<String> = key
                        .split('/')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                        .collect();
                    (subtree, segs)
                } else {
                    // Stale path — clear the filter and fall back to the
                    // full tree so the user isn't stuck on an empty
                    // listing.
                    state.selected_folder = None;
                    (full_tree.clone(), Vec::new())
                }
            } else {
                (full_tree.clone(), Vec::new())
            };
        if let Some(key) = state.selected_folder.clone() {
            ui.horizontal(|ui| {
                ui.label("Filtered to:");
                ui.code(&key);
                if ui
                    .button("Show all folders")
                    .on_hover_text("Clear the folder filter and show every build.")
                    .clicked()
                {
                    state.selected_folder = None;
                }
            });
            ui.separator();
        }
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .max_height(360.0)
            .show(ui, |ui| {
                let mut path: Vec<&str> = root_path_segments.iter().map(String::as_str).collect();
                render_folder(ui, &tree, &mut path, state, &mut action, current_path);
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
        // Issue #213 (slice 5): "Move to folder…" popup. Sources its
        // folder list from the *full* tree (not `tree`, which may be
        // filtered) so the user can move a build to any folder —
        // including out of the active folder filter — and so the
        // emitted target paths are full-tree-relative.
        render_move_popup(ui, state, &full_tree, &mut action);
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
    current_path: Option<&std::path::Path>,
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
                render_folder(ui, child, path, state, action, current_path);
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
            // Issue #213 (folder-isolation slice): toggle "Show only
            // this folder" / "Show all folders" depending on whether
            // the user has filtered to this exact path. Picked up by
            // the chip row at the top of the tree on next frame.
            if state.selected_folder.as_deref() == Some(key.as_str()) {
                if ui.button("Show all folders").clicked() {
                    state.selected_folder = None;
                    ui.close_menu();
                }
            } else if ui.button("Show only this folder").clicked() {
                state.selected_folder = Some(key.clone());
                ui.close_menu();
            }
            ui.separator();
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
        render_build_row(ui, entry, state, action, current_path);
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
/// Issue #213 follow-up: format the time delta between `now` and `then`
/// as a human-readable "X ago" string. Pure / no I/O so the threshold
/// rules are unit-testable.
///
/// Returns `(future)` when `then > now` (defensive — clock skew on the
/// filesystem can yield a later mtime than the system clock; rendering
/// `-1 minutes ago` reads worse than the explicit marker).
///
/// Resolution bands:
/// - <60s → "just now"
/// - <60m → "<N> min ago" (singular "1 min ago")
/// - <24h → "<N> hours ago" (singular "1 hour ago")
/// - <30d → "<N> days ago" (singular "1 day ago")
/// - otherwise → "<N> months ago" (singular "1 month ago"), capped at
///   `12 months ago` for anything older.
#[must_use]
pub fn format_relative_time(now: std::time::SystemTime, then: std::time::SystemTime) -> String {
    let Ok(elapsed) = now.duration_since(then) else {
        return "(future)".to_owned();
    };
    let secs = elapsed.as_secs();
    if secs < 60 {
        return "just now".to_owned();
    }
    let mins = secs / 60;
    if mins < 60 {
        return if mins == 1 {
            "1 min ago".to_owned()
        } else {
            format!("{mins} min ago")
        };
    }
    let hours = mins / 60;
    if hours < 24 {
        return if hours == 1 {
            "1 hour ago".to_owned()
        } else {
            format!("{hours} hours ago")
        };
    }
    let days = hours / 24;
    if days < 30 {
        return if days == 1 {
            "1 day ago".to_owned()
        } else {
            format!("{days} days ago")
        };
    }
    let months = (days / 30).min(12);
    if months == 1 {
        "1 month ago".to_owned()
    } else {
        format!("{months} months ago")
    }
}

/// Issue #213 follow-up: choice picked from the build-row right-click
/// context menu. Each variant matches one of the inline action buttons
/// in [`render_build_row`] — keeping the surface enum-shaped lets the
/// menu wiring stay one match arm and gives tests a way to exercise
/// the state mutations without spinning up egui.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildRowMenuChoice {
    Rename,
    Duplicate,
    /// Open the move-to-folder picker popup. Currently disk-only — the
    /// helper makes it a no-op on other backends so the renderer can
    /// always offer the menu item and the host-side rename support
    /// landing later auto-extends it.
    MoveToFolder,
    /// First click on Delete — arm the two-step confirmation.
    DeleteRequest,
    /// Second click on Delete — actually emit the action.
    DeleteConfirm,
}

/// Issue #213 follow-up: apply a context-menu pick to the builds-tab
/// state and (where appropriate) emit a [`BuildsAction`]. Mirrors the
/// inline-button handlers in [`render_build_row`] one-for-one so both
/// affordances stay in sync. Pure-ish: touches state + action only.
pub fn apply_build_row_menu_choice(
    choice: BuildRowMenuChoice,
    entry: &BuildEntry,
    state: &mut BuildsTabState,
    action: &mut Option<BuildsAction>,
) {
    match choice {
        BuildRowMenuChoice::Rename => {
            state.pending_rename = Some((entry.id.clone(), entry.label.clone()));
            state.pending_delete = None;
        }
        BuildRowMenuChoice::Duplicate => {
            *action = Some(BuildsAction::Duplicate(entry.id.clone()));
            state.loaded = false;
        }
        BuildRowMenuChoice::MoveToFolder => {
            if matches!(entry.id, BuildId::Disk(_)) {
                state.move_popup_for = Some(entry.id.clone());
                state.pending_delete = None;
            }
        }
        BuildRowMenuChoice::DeleteRequest => {
            state.pending_delete = Some(entry.id.clone());
        }
        BuildRowMenuChoice::DeleteConfirm => {
            *action = Some(BuildsAction::Delete(entry.id.clone()));
            state.pending_delete = None;
            state.loaded = false;
        }
    }
}

/// Issue #213 follow-up: shape the displayed row label so the
/// currently-loaded build gets a leading `●` marker. Pure helper —
/// the renderer also bumps the `RichText` to strong, but that lives
/// at the egui layer; this exists so the prefix rule is documented +
/// pinned by a unit test (see `current_build_row_label_*`). Without
/// this seam an earlier draft of the PR plumbed `is_current_build`
/// to every layer but forgot to actually use it in `render_build_row`
/// — the seam makes that regression noisy.
#[must_use]
pub fn current_build_row_label(label: &str, is_current: bool) -> String {
    if is_current {
        format!("● {label}")
    } else {
        label.to_owned()
    }
}

/// Issue #213 follow-up: tell whether `entry` is the build currently
/// loaded into the app. Only matches `BuildId::Disk` entries since
/// the wasm IDB / FSA-folder variants don't carry a comparable
/// `Path` today. Pure helper so the marker rule is documented + the
/// edge cases (`None` → never, wasm-only entries → never) are
/// testable in isolation.
#[must_use]
pub fn is_current_build(entry: &BuildEntry, current: Option<&std::path::Path>) -> bool {
    let Some(current) = current else {
        return false;
    };
    match &entry.id {
        BuildId::Disk(p) => p == current,
        BuildId::Idb(_) | BuildId::Folder(_) => false,
    }
}

/// delete controls). Extracted so both the root level and every
/// nested folder share one code path.
fn render_build_row(
    ui: &mut egui::Ui,
    entry: &BuildEntry,
    state: &mut BuildsTabState,
    action: &mut Option<BuildsAction>,
    current_path: Option<&std::path::Path>,
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
        } else {
            // Issue #213 follow-up: when the backend supplied an
            // mtime, append "Modified X ago" to the hover so users
            // can spot recently-touched builds without checking the
            // filesystem.
            // Issue #213 follow-up: surface the build that's currently
            // loaded so the user can find it at a glance. The renderer
            // delegates to `current_build_row_label` (pure helper, see
            // tests) so a future refactor can't silently drop the
            // wiring like an earlier draft did.
            let is_current = is_current_build(entry, current_path);
            let label_text = current_build_row_label(&entry.label, is_current);
            let mut rich = egui::RichText::new(label_text).monospace();
            if is_current {
                rich = rich.strong();
            }
            let hover_text = if let Some(mtime) = entry.modified {
                format!(
                    "Click to load · right-click for actions\nModified {}",
                    format_relative_time(std::time::SystemTime::now(), mtime),
                )
            } else {
                "Click to load · right-click for actions".to_owned()
            };
            let label_resp = ui
                .add(egui::Label::new(rich).sense(egui::Sense::click()))
                .on_hover_text(hover_text);
            if label_resp.clicked() {
                *action = Some(BuildsAction::Load(entry.id.clone()));
            }
            // Issue #213 follow-up: right-click context menu mirroring
            // the inline action buttons. Discoverable for users who
            // expect a desktop-app convention and faster than reaching
            // for the row's icon strip on dense lists.
            let pending_delete_for_this = state
                .pending_delete
                .as_ref()
                .map(|id| id == &entry.id)
                .unwrap_or(false);
            let is_disk = matches!(entry.id, BuildId::Disk(_));
            label_resp.context_menu(|ui| {
                if ui.button("Rename").clicked() {
                    apply_build_row_menu_choice(BuildRowMenuChoice::Rename, entry, state, action);
                    ui.close_menu();
                }
                if ui.button("Duplicate").clicked() {
                    apply_build_row_menu_choice(
                        BuildRowMenuChoice::Duplicate,
                        entry,
                        state,
                        action,
                    );
                    ui.close_menu();
                }
                if ui
                    .add_enabled(is_disk, egui::Button::new("Move to folder…"))
                    .on_disabled_hover_text("Move-to-folder is currently disk-only.")
                    .clicked()
                {
                    apply_build_row_menu_choice(
                        BuildRowMenuChoice::MoveToFolder,
                        entry,
                        state,
                        action,
                    );
                    ui.close_menu();
                }
                ui.separator();
                let delete_label = if pending_delete_for_this {
                    "Confirm delete"
                } else {
                    "Delete…"
                };
                let delete_color = if pending_delete_for_this {
                    egui::Color32::from_rgb(0xDD, 0x00, 0x22)
                } else {
                    ui.visuals().text_color()
                };
                if ui
                    .button(egui::RichText::new(delete_label).color(delete_color))
                    .clicked()
                {
                    let choice = if pending_delete_for_this {
                        BuildRowMenuChoice::DeleteConfirm
                    } else {
                        BuildRowMenuChoice::DeleteRequest
                    };
                    apply_build_row_menu_choice(choice, entry, state, action);
                    ui.close_menu();
                }
            });
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
            // Issue #213 (slice 5): "Move to folder…" affordance.
            // Opens a popup (rendered once per frame in
            // `render_move_popup`) listing every folder currently in
            // the listing. Disk-only for now — wasm and folder-backed
            // sources don't yet support `fs::rename`-like move.
            if matches!(entry.id, BuildId::Disk(_))
                && ui
                    .small_button("⇨")
                    .on_hover_text("Move to folder…")
                    .clicked()
            {
                state.move_popup_for = Some(entry.id.clone());
                state.pending_delete = None;
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

/// Issue #213 (slice 5): every folder path in `tree`, root included,
/// flattened depth-first and slash-joined. The root is represented by
/// the empty string, mirroring [`folder_path_key`].
///
/// Pure helper extracted so the "Move to folder…" popup has a
/// testable input model and so the same path-walking rule is shared
/// between the renderer and any future "validate target folder
/// exists" check.
#[must_use]
pub fn collect_folder_paths(tree: &FolderNode) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack: Vec<&str> = Vec::new();
    walk(tree, &mut stack, &mut out);
    return out;

    fn walk<'a>(node: &'a FolderNode, stack: &mut Vec<&'a str>, out: &mut Vec<String>) {
        out.push(stack.join("/"));
        for child in &node.children {
            stack.push(child.name.as_str());
            walk(child, stack, out);
            stack.pop();
        }
    }
}

/// Issue #213 (slice 5): render the "Move to folder…" popup. Lists
/// every folder the renderer knows about so the user can pick any
/// target without first having to expand the tree. Picking a target
/// emits `BuildsAction::MoveBuild` and closes the popup; Cancel /
/// outside-click closes without emitting.
fn render_move_popup(
    ui: &mut egui::Ui,
    state: &mut BuildsTabState,
    tree: &FolderNode,
    action: &mut Option<BuildsAction>,
) {
    let Some(target_id) = state.move_popup_for.clone() else {
        return;
    };
    // Resolve the source build's current folder for display + to skip
    // the "move to where I already am" option.
    let current_folder = state
        .entries
        .iter()
        .find(|e| e.id == target_id)
        .and_then(|e| e.category.clone());
    let current_label = state
        .entries
        .iter()
        .find(|e| e.id == target_id)
        .map_or_else(|| "build".to_owned(), |e| e.label.clone());

    let mut open = true;
    let mut close = false;
    let mut chosen_target: Option<Option<String>> = None;
    egui::Window::new("Move to folder")
        .id(egui::Id::new("builds-move-popup"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ui.ctx(), |ui| {
            ui.label(format!("Move \"{current_label}\" to:"));
            let folders = collect_folder_paths(tree);
            egui::ScrollArea::vertical()
                .id_salt("move-popup-list")
                .max_height(280.0)
                .show(ui, |ui| {
                    for path in folders {
                        // The empty key represents the root. The
                        // current folder is unselectable so users
                        // can't issue a no-op move.
                        let is_root = path.is_empty();
                        let is_current = match (&current_folder, is_root) {
                            (None, true) => true,
                            (Some(cur), false) => cur == &path,
                            _ => false,
                        };
                        let label = if is_root {
                            "Root".to_owned()
                        } else {
                            path.clone()
                        };
                        let display = if is_current {
                            format!("● {label} (current)")
                        } else {
                            format!("  {label}")
                        };
                        if ui
                            .add_enabled(!is_current, egui::Button::new(display))
                            .clicked()
                        {
                            chosen_target = Some(if is_root { None } else { Some(path.clone()) });
                        }
                    }
                });
            ui.add_space(4.0);
            if ui.button("Cancel").clicked() {
                close = true;
            }
        });

    if let Some(target) = chosen_target {
        *action = Some(BuildsAction::MoveBuild {
            id: target_id,
            target,
        });
        state.move_popup_for = None;
        state.loaded = false;
    } else if close || !open {
        state.move_popup_for = None;
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
            modified: None,
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

    #[test]
    fn collect_folder_paths_lists_root_and_descendants() {
        // Issue #213 (slice 5): the move-to-folder popup needs every
        // folder in the tree as a target option. Root is `""`; nested
        // folders use slash-joined names.
        let entries = vec![
            entry("a", None),
            entry("b", Some("Levelling")),
            entry("c", Some("Levelling/Marauder")),
            entry("d", Some("Bossing")),
        ];
        let tree = build_folder_tree(&entries);
        let mut paths = collect_folder_paths(&tree);
        paths.sort();
        assert_eq!(
            paths,
            vec![
                String::new(),
                "Bossing".to_owned(),
                "Levelling".to_owned(),
                "Levelling/Marauder".to_owned(),
            ],
        );
    }

    #[test]
    fn collect_folder_paths_on_empty_tree_returns_just_root() {
        // An empty listing still produces the root as a target, so the
        // user can "move into the root" even before any folders exist
        // (technically a no-op for a build that's already there, but
        // the popup handles the no-op case explicitly).
        let tree = build_folder_tree(&[]);
        assert_eq!(collect_folder_paths(&tree), vec![String::new()]);
    }

    #[test]
    fn format_relative_time_just_now_under_a_minute() {
        let now = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000);
        let then = now - std::time::Duration::from_secs(30);
        assert_eq!(format_relative_time(now, then), "just now");
    }

    #[test]
    fn format_relative_time_minutes_singular_and_plural() {
        let now = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000);
        let one_min = now - std::time::Duration::from_secs(60);
        assert_eq!(format_relative_time(now, one_min), "1 min ago");
        let five_min = now - std::time::Duration::from_secs(60 * 5);
        assert_eq!(format_relative_time(now, five_min), "5 min ago");
    }

    #[test]
    fn format_relative_time_hours_singular_and_plural() {
        let now = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000);
        let one_hour = now - std::time::Duration::from_secs(3600);
        assert_eq!(format_relative_time(now, one_hour), "1 hour ago");
        let three_hours = now - std::time::Duration::from_secs(3600 * 3);
        assert_eq!(format_relative_time(now, three_hours), "3 hours ago");
    }

    #[test]
    fn format_relative_time_days_singular_and_plural() {
        let now = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(86_400 * 60);
        let one_day = now - std::time::Duration::from_secs(86_400);
        assert_eq!(format_relative_time(now, one_day), "1 day ago");
        let five_days = now - std::time::Duration::from_secs(86_400 * 5);
        assert_eq!(format_relative_time(now, five_days), "5 days ago");
    }

    #[test]
    fn format_relative_time_months_and_cap_at_twelve() {
        let now =
            std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(86_400 * 365 * 5);
        let two_months = now - std::time::Duration::from_secs(86_400 * 60);
        assert_eq!(format_relative_time(now, two_months), "2 months ago");
        // Anything older than 12 months caps to avoid runaway numbers.
        let three_years = now - std::time::Duration::from_secs(86_400 * 365 * 3);
        assert_eq!(format_relative_time(now, three_years), "12 months ago");
    }

    #[test]
    fn format_relative_time_future_falls_back_to_marker() {
        // Filesystem clock skew can produce a later mtime than the
        // system clock — don't render "-1 minutes ago".
        let now = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000);
        let later = now + std::time::Duration::from_secs(60);
        assert_eq!(format_relative_time(now, later), "(future)");
    }

    #[test]
    fn is_current_build_matches_disk_path_exactly() {
        // The build whose `BuildId::Disk` path equals the loaded
        // `current_path` is the active build.
        let e = entry("MyBuild", None);
        let path = PathBuf::from("/tmp/MyBuild.mk2");
        assert!(is_current_build(&e, Some(&path)));
    }

    #[test]
    fn is_current_build_returns_false_for_unrelated_path() {
        let e = entry("MyBuild", None);
        let other = PathBuf::from("/tmp/SomethingElse.mk2");
        assert!(!is_current_build(&e, Some(&other)));
    }

    #[test]
    fn is_current_build_returns_false_when_no_current_path() {
        // No build loaded → no row should be marked. Distinct from
        // "loaded build doesn't match" so the renderer can short-
        // circuit cleanly.
        let e = entry("MyBuild", None);
        assert!(!is_current_build(&e, None));
    }

    #[test]
    fn is_current_build_returns_false_for_wasm_only_entries() {
        // `BuildId::Idb` / `BuildId::Folder` don't carry a comparable
        // path, so the marker stays off — wasm parity is a follow-up.
        let path = PathBuf::from("/tmp/MyBuild.mk2");
        let idb_entry = BuildEntry {
            label: "WasmBuild".to_owned(),
            id: BuildId::Idb("uuid-abc".to_owned()),
            ext: "mk2".to_owned(),
            category: None,
            modified: None,
        };
        assert!(!is_current_build(&idb_entry, Some(&path)));
        let folder_entry = BuildEntry {
            label: "FsaBuild".to_owned(),
            id: BuildId::Folder("file.mk2".to_owned()),
            ext: "mk2".to_owned(),
            category: None,
            modified: None,
        };
        assert!(!is_current_build(&folder_entry, Some(&path)));
    }

    #[test]
    fn current_build_row_label_prefixes_when_current() {
        // The "current" row gets a leading "● " marker so the user
        // can find it at a glance in a dense list.
        assert_eq!(current_build_row_label("MyBuild", true), "● MyBuild");
    }

    #[test]
    fn current_build_row_label_passthrough_when_not_current() {
        // Every other row keeps its plain label so non-current builds
        // line up column-wise with the marker on the active one.
        assert_eq!(current_build_row_label("MyBuild", false), "MyBuild");
    }

    #[test]
    fn current_build_row_label_handles_empty_label() {
        // Defensive — an empty label string still gets the marker if
        // it's the current build, so the renderer never silently
        // hides which row is active.
        assert_eq!(current_build_row_label("", true), "● ");
        assert_eq!(current_build_row_label("", false), "");
    }

    #[test]
    fn build_row_menu_rename_arms_pending_rename() {
        // Issue #213 follow-up: right-click → Rename mirrors the
        // inline ✎ button — opens the inline-rename TextEdit for the
        // clicked row and clears any in-flight delete confirmation
        // so two different rows can't be ambiguously queued.
        let mut state = BuildsTabState::default();
        let other_id = BuildId::Disk(PathBuf::from("/tmp/other.mk2"));
        state.pending_delete = Some(other_id);
        let e = entry("MyBuild", None);
        let mut action: Option<BuildsAction> = None;
        apply_build_row_menu_choice(BuildRowMenuChoice::Rename, &e, &mut state, &mut action);
        assert_eq!(
            state.pending_rename,
            Some((e.id.clone(), "MyBuild".to_owned()))
        );
        assert!(
            state.pending_delete.is_none(),
            "Rename should clear any in-flight delete on another row",
        );
        assert!(
            action.is_none(),
            "Rename arms state, doesn't emit an action"
        );
    }

    #[test]
    fn build_row_menu_duplicate_emits_action() {
        // Mirrors the inline ⎘ button. Emitting the action prompts the
        // host to allocate a `<name> copy.<ext>` and re-list; the
        // renderer also drops `loaded` so the next frame requests a
        // refresh once the host writes the new file.
        let mut state = BuildsTabState::default();
        state.loaded = true;
        let e = entry("MyBuild", None);
        let mut action: Option<BuildsAction> = None;
        apply_build_row_menu_choice(BuildRowMenuChoice::Duplicate, &e, &mut state, &mut action);
        assert_eq!(action, Some(BuildsAction::Duplicate(e.id.clone())));
        assert!(!state.loaded, "Duplicate marks the listing stale");
    }

    #[test]
    fn build_row_menu_move_opens_popup_for_disk_builds() {
        // Mirrors the inline ⇨ button. Move-to-folder is disk-only;
        // wasm IndexedDB / FSA-folder backends don't support rename
        // yet so the popup wouldn't apply cleanly.
        let mut state = BuildsTabState::default();
        let e = entry("MyBuild", None);
        let mut action: Option<BuildsAction> = None;
        apply_build_row_menu_choice(
            BuildRowMenuChoice::MoveToFolder,
            &e,
            &mut state,
            &mut action,
        );
        assert_eq!(state.move_popup_for, Some(e.id.clone()));
        assert!(action.is_none());
    }

    #[test]
    fn build_row_menu_move_is_noop_for_non_disk_builds() {
        let mut state = BuildsTabState::default();
        let e = BuildEntry {
            label: "WasmBuild".to_owned(),
            id: BuildId::Idb("uuid-abc".to_owned()),
            ext: "mk2".to_owned(),
            category: None,
            modified: None,
        };
        let mut action: Option<BuildsAction> = None;
        apply_build_row_menu_choice(
            BuildRowMenuChoice::MoveToFolder,
            &e,
            &mut state,
            &mut action,
        );
        assert!(
            state.move_popup_for.is_none(),
            "wasm builds can't be moved yet — the menu pick should be a no-op",
        );
    }

    #[test]
    fn build_row_menu_delete_is_two_step() {
        // Mirrors the inline 🗑 → red Confirm flow. The first click
        // arms `pending_delete`; only the second emits the action so
        // an accidental right-click doesn't lose a build.
        let mut state = BuildsTabState::default();
        let e = entry("MyBuild", None);
        let mut action: Option<BuildsAction> = None;
        apply_build_row_menu_choice(
            BuildRowMenuChoice::DeleteRequest,
            &e,
            &mut state,
            &mut action,
        );
        assert_eq!(state.pending_delete, Some(e.id.clone()));
        assert!(action.is_none(), "First click only arms confirmation");
        apply_build_row_menu_choice(
            BuildRowMenuChoice::DeleteConfirm,
            &e,
            &mut state,
            &mut action,
        );
        assert_eq!(action, Some(BuildsAction::Delete(e.id.clone())));
        assert!(
            state.pending_delete.is_none(),
            "Confirmed delete clears the arm"
        );
    }

    #[test]
    fn move_popup_must_use_full_tree_not_filtered_subtree() {
        // Regression: when the folder-isolation filter is active the
        // renderer scopes the visible tree to a subtree via
        // `filter_folder_to_subtree`, but the move-to-folder popup
        // must still see the *full* hierarchy. Otherwise (a) the user
        // can't move a build out of the active filter and (b) emitted
        // target paths are root-relative to the subtree, producing
        // wrong destinations like "Sirus" instead of "Bossing/Sirus".
        let entries = vec![
            entry("a", Some("Bossing")),
            entry("b", Some("Bossing/Sirus")),
            entry("c", Some("Levelling")),
        ];
        let full = build_folder_tree(&entries);
        let filtered = crate::builds_folder_tree::filter_folder_to_subtree(&full, "Bossing")
            .expect("Bossing subtree");

        let mut full_paths = collect_folder_paths(&full);
        full_paths.sort();
        assert!(
            full_paths.contains(&"Bossing/Sirus".to_owned()),
            "full tree must expose the nested 'Bossing/Sirus' target",
        );
        assert!(
            full_paths.contains(&"Levelling".to_owned()),
            "full tree must expose folders outside the active filter \
             so the user can move builds out of the filter",
        );

        let mut filtered_paths = collect_folder_paths(&filtered);
        filtered_paths.sort();
        assert!(
            !filtered_paths.contains(&"Bossing/Sirus".to_owned()),
            "filtered subtree truncates nested paths — passing it to \
             the move popup would emit the wrong target path",
        );
        assert!(
            !filtered_paths.contains(&"Levelling".to_owned()),
            "filtered subtree hides sibling folders — passing it to \
             the move popup would strand builds inside the filter",
        );
    }
}
