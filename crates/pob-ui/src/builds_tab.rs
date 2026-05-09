//! Builds tab — UI shell for the cross-platform build browser.
//!
//! On desktop the host backs this with the platform-specific builds
//! directory (see [`build_store_disk`]). On wasm the host backs it
//! with [`build_store_wasm`], which stores `.mk2` payloads in
//! IndexedDB and optionally mirrors a real folder via the File System
//! Access API. Either way, this module is concerned only with
//! rendering the listing + emitting [`BuildsAction`]s; it never
//! touches storage directly.

use std::path::PathBuf;

use eframe::egui;

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
        // Issue #100 (slice 2): group entries by category so users
        // can collapse rare-use folders. Entries with no category
        // land under "Uncategorised". `state.entries` is assumed to
        // be category-then-label sorted by the host.
        let mut groups: Vec<(Option<String>, Vec<usize>)> = Vec::new();
        let entries_clone: Vec<BuildEntry> = state.entries.clone();
        for (i, entry) in entries_clone.iter().enumerate() {
            match groups.last_mut() {
                Some((cat, items)) if cat.as_deref() == entry.category.as_deref() => {
                    items.push(i);
                }
                _ => groups.push((entry.category.clone(), vec![i])),
            }
        }

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .max_height(360.0)
            .show(ui, |ui| {
                for (cat, indices) in &groups {
                    let header = cat.as_deref().unwrap_or("Uncategorised");
                    egui::CollapsingHeader::new(format!("{header}  ({n})", n = indices.len()))
                        .default_open(true)
                        .id_salt(format!("builds_cat_{header}"))
                        .show(ui, |ui| {
                            for &i in indices {
                                let entry = &entries_clone[i];
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
                                        let confirm = ui
                                            .button("OK")
                                            .on_hover_text("Apply rename")
                                            .clicked()
                                            || (resp.lost_focus()
                                                && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                                        let cancel =
                                            ui.button("✕").on_hover_text("Cancel rename").clicked();
                                        if confirm {
                                            let new_name = buf.trim().to_owned();
                                            if !new_name.is_empty() && new_name != entry.label {
                                                action = Some(BuildsAction::Rename {
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
                                            egui::Label::new(
                                                egui::RichText::new(&entry.label).monospace(),
                                            )
                                            .sense(egui::Sense::click()),
                                        )
                                        .on_hover_text("Click to load")
                                        .clicked()
                                    {
                                        action = Some(BuildsAction::Load(entry.id.clone()));
                                    }

                                    ui.weak(format!(".{}", entry.ext));

                                    if !renaming_this {
                                        if ui.small_button("✎").on_hover_text("Rename").clicked()
                                        {
                                            state.pending_rename =
                                                Some((entry.id.clone(), entry.label.clone()));
                                            state.pending_delete = None;
                                        }
                                        if ui.small_button("⎘").on_hover_text("Duplicate").clicked()
                                        {
                                            action =
                                                Some(BuildsAction::Duplicate(entry.id.clone()));
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
                                                        egui::RichText::new("Confirm?").color(
                                                            egui::Color32::from_rgb(
                                                                0xDD, 0x00, 0x22,
                                                            ),
                                                        ),
                                                    )
                                                    .small(),
                                                )
                                                .on_hover_text("Click again to permanently delete")
                                                .clicked()
                                            {
                                                action =
                                                    Some(BuildsAction::Delete(entry.id.clone()));
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
                        });
                }
            });
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
