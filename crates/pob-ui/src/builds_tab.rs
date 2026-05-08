//! Builds tab — disk-backed build browser. Resolves a platform-
//! specific builds directory at startup, lists every `.mk2` and
//! `.xml` build file in it, and hands click events back to the host
//! so it can run its existing load path.
//!
//! Intentionally minimal — no auto-save and no category subdirs yet
//! (PoB has those; they're tracked as #37 follow-ups). A non-wasm
//! build that can't resolve a builds directory falls through to a
//! "no builds folder available" message rather than panicking.

use eframe::egui;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default)]
pub struct BuildsTabState {
    /// Most recent listing of the builds folder. Populated lazily on
    /// the first frame and refreshed via the "Refresh" button or
    /// after the user saves a new build.
    pub entries: Vec<BuildEntry>,
    /// Whether we've populated `entries` at least once. Avoids
    /// repeated I/O on every frame.
    pub loaded: bool,
    /// Buffer for the "save current as <name>" input.
    pub save_buffer: String,
    /// Issue #100 (slice 3): inline rename state. When `Some`, the
    /// matching build entry renders a TextEdit in place of its
    /// label until the user confirms or cancels.
    pub pending_rename: Option<(PathBuf, String)>,
    /// Issue #100 (slice 3): two-step delete confirmation. The first
    /// click on Delete records the path here; the second click on
    /// the same row's now-red "Confirm?" button fires the action.
    /// Cleared if the user clicks anywhere else (rescan + frame
    /// reset cycle).
    pub pending_delete: Option<PathBuf>,
    /// Issue #100 (slice 3): buffer for the "+ New category" input.
    pub new_category_buffer: String,
}

#[derive(Debug, Clone)]
pub struct BuildEntry {
    /// File-stem display label (e.g. "MyBuild").
    pub label: String,
    /// Absolute path to the file on disk.
    pub path: PathBuf,
    /// File extension lowercased (`mk2` / `xml`).
    pub ext: String,
    /// Issue #100 (slice 2): subdirectory the build lives under,
    /// relative to the builds root. `None` means the file is in the
    /// root itself ("Uncategorised" group). Mirrors PoB's
    /// "Levelling/", "Bossing/", "Mapping/" folder convention. We
    /// only walk one level deep — nested categories are out of scope.
    pub category: Option<String>,
}

/// Action the host should take based on the user's interaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildsAction {
    /// User asked to load this file. The host runs its normal load
    /// path and updates the live build.
    LoadFile(PathBuf),
    /// User asked to save the current build to this filename inside
    /// the builds dir. Host calls its save path with the resolved
    /// absolute path.
    SaveAs(PathBuf),
    /// User clicked "Open builds folder" — host should open the
    /// directory in the platform file manager.
    OpenFolder(PathBuf),
    /// Issue #100 (slice 3): rename a build on disk. Host issues
    /// `fs::rename(from, to)` and refreshes the listing. If the
    /// renamed file was the currently-open build the host should
    /// also update its `current_build_path`.
    Rename { from: PathBuf, to: PathBuf },
    /// Issue #100 (slice 3): duplicate a build via `fs::copy(from,
    /// to)`. The destination defaults to `<name> copy.<ext>` in the
    /// same category subdir.
    Duplicate { from: PathBuf, to: PathBuf },
    /// Issue #100 (slice 3): permanently delete the named build via
    /// `fs::remove_file(path)`. Surfaced from a two-step confirm in
    /// the UI; the host doesn't need to ask again.
    Delete(PathBuf),
    /// Issue #100 (slice 3): create an empty category subdirectory
    /// under the builds root. Host issues `fs::create_dir_all(dir)`
    /// and refreshes.
    CreateCategory(PathBuf),
}

/// Resolve the platform-specific builds directory. Returns `None` if
/// the environment doesn't carry the relevant home/appdata variable
/// (test runners, daemons running as root, etc.). Mirrors PoB's
/// `~/Path of Building/Builds/` on Linux and the equivalents on macOS
/// / Windows but uses our app name so we don't collide with upstream
/// PoB's installation.
#[must_use]
pub fn builds_dir() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var_os("HOME")?;
        let mut p = PathBuf::from(home);
        p.push("Library");
        p.push("Application Support");
        p.push("PathOfBuildingMK2");
        p.push("Builds");
        Some(p)
    }
    #[cfg(target_os = "linux")]
    {
        let base = std::env::var_os("XDG_DATA_HOME").map_or_else(
            || {
                std::env::var_os("HOME").map(|h| {
                    let mut p = PathBuf::from(h);
                    p.push(".local");
                    p.push("share");
                    p
                })
            },
            |x| Some(PathBuf::from(x)),
        )?;
        let mut p = base;
        p.push("PathOfBuildingMK2");
        p.push("Builds");
        Some(p)
    }
    #[cfg(target_os = "windows")]
    {
        let appdata = std::env::var_os("APPDATA")?;
        let mut p = PathBuf::from(appdata);
        p.push("PathOfBuildingMK2");
        p.push("Builds");
        Some(p)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        None
    }
}

/// Refresh the entry list from disk. Creates the builds dir if it
/// doesn't exist (so the user has somewhere to save into). Issue
/// #100 (slice 2): walks one level of subdirectories so the user
/// can organise builds into categories (e.g. `Levelling/MyAcrobat`,
/// `Bossing/HC-DD`). Files in the root itself are tagged with
/// `category = None` and render under "Uncategorised". Nested
/// subdirectories are flattened — only the immediate parent of each
/// build file becomes a category label.
fn rescan(dir: &Path) -> Vec<BuildEntry> {
    let _ = std::fs::create_dir_all(dir);
    let mut out: Vec<BuildEntry> = Vec::new();
    if let Ok(read) = std::fs::read_dir(dir) {
        for entry in read.filter_map(|e| e.ok()) {
            let path = entry.path();
            let file_type = entry.file_type().ok();
            if file_type.map(|t| t.is_dir()).unwrap_or(false) {
                // One-level-deep walk into subdirectories. Skip
                // hidden / metadata dirs (anything starting with
                // `.`) so cache directories don't pollute the list.
                let category = path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .filter(|s| !s.starts_with('.'))
                    .map(str::to_owned);
                let Some(category) = category else { continue };
                if let Ok(child) = std::fs::read_dir(&path) {
                    for f in child.filter_map(|e| e.ok()) {
                        if let Some(be) = build_entry_from_path(f.path(), Some(category.clone()))
                        {
                            out.push(be);
                        }
                    }
                }
            } else if let Some(be) = build_entry_from_path(path, None) {
                out.push(be);
            }
        }
    }
    out.sort_by(|a, b| {
        // Stable category-then-label ordering keeps the rendered
        // groups deterministic. `None` (Uncategorised) sorts first
        // so users see their root-level builds without scrolling.
        let ca = a.category.as_deref().unwrap_or("");
        let cb = b.category.as_deref().unwrap_or("");
        let by_cat = match (a.category.is_some(), b.category.is_some()) {
            (false, true) => std::cmp::Ordering::Less,
            (true, false) => std::cmp::Ordering::Greater,
            _ => ca
                .to_ascii_lowercase()
                .cmp(&cb.to_ascii_lowercase()),
        };
        by_cat.then_with(|| {
            a.label
                .to_ascii_lowercase()
                .cmp(&b.label.to_ascii_lowercase())
        })
    });
    out
}

/// Issue #100 (slice 3): pick an unused destination path for a
/// duplicate of `entry`. Tries `<name> copy.<ext>` first, then
/// `<name> copy 2.<ext>`, etc. until it finds one that doesn't
/// already exist on disk. Returns `None` if `entry.path` has no
/// parent directory (shouldn't happen for a real build file).
fn duplicate_target(entry: &BuildEntry) -> Option<PathBuf> {
    let parent = entry.path.parent()?;
    for n in 1..100 {
        let suffix = if n == 1 {
            "copy".to_owned()
        } else {
            format!("copy {n}")
        };
        let candidate = parent.join(format!("{} {}.{}", entry.label, suffix, entry.ext));
        if !candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

/// Helper for `rescan`: build an `Option<BuildEntry>` from a
/// candidate path, filtering out non-build extensions and rejecting
/// paths whose stem doesn't decode as UTF-8.
fn build_entry_from_path(path: PathBuf, category: Option<String>) -> Option<BuildEntry> {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase())?;
    if ext != "mk2" && ext != "xml" {
        return None;
    }
    let label = path.file_stem().and_then(|s| s.to_str())?.to_owned();
    Some(BuildEntry {
        label,
        path,
        ext,
        category,
    })
}

/// Render the builds tab. Returns an action for the host to execute
/// (load file, save current under a name, open folder), or `None` if
/// the user only browsed.
pub fn ui(ui: &mut egui::Ui, state: &mut BuildsTabState) -> Option<BuildsAction> {
    let Some(dir) = builds_dir() else {
        ui.heading("Builds");
        ui.separator();
        ui.label(
            "Couldn't resolve a builds directory for this platform. Set $HOME (or %APPDATA% \
             on Windows) to enable disk-backed builds.",
        );
        return None;
    };

    if !state.loaded {
        state.entries = rescan(&dir);
        state.loaded = true;
    }

    let mut action: Option<BuildsAction> = None;

    ui.horizontal(|ui| {
        ui.heading("Builds");
        ui.separator();
        if ui.button("Refresh").clicked() {
            state.entries = rescan(&dir);
        }
        if ui.button("Open folder").clicked() {
            action = Some(BuildsAction::OpenFolder(dir.clone()));
        }
    });
    ui.weak(format!("Folder: {}", dir.display()));

    // Issue #100 (slice 3): "+ New category" creates an empty
    // subdirectory under the builds root so the user can drop saves
    // into it via the regular Save flow next time.
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
            .on_hover_text(
                "Creates an empty subdirectory under the builds root. \
                 Drop saves into it via Save here later.",
            )
            .clicked()
        {
            let mut path = dir.clone();
            path.push(trimmed);
            action = Some(BuildsAction::CreateCategory(path));
            state.new_category_buffer.clear();
            state.loaded = false;
        }
    });
    ui.separator();

    if state.entries.is_empty() {
        ui.label(
            "No builds saved yet. Save the current build into this folder via \
             \"Save here\" below — or use File → Save As to pick any path manually.",
        );
    } else {
        // Issue #100 (slice 2): group entries by category so users
        // can collapse rare-use folders. Entries in the root land
        // under "Uncategorised"; every named subdir gets its own
        // collapsing header. `state.entries` is already
        // category-then-label sorted, so we can stream straight
        // through it.
        // Pre-compute the (category → entry index) groupings once so
        // the render loop can stream through ScrollArea without
        // borrowing tricks. `state.entries` is already
        // category-then-label sorted from rescan.
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
                    egui::CollapsingHeader::new(format!(
                        "{header}  ({n})",
                        n = indices.len()
                    ))
                    .default_open(true)
                    .id_salt(format!("builds_cat_{header}"))
                    .show(ui, |ui| {
                        for &i in indices {
                            let entry = &entries_clone[i];
                            // Issue #100 (slice 3): per-row controls.
                            // Either an inline rename TextEdit or the
                            // standard click-to-load label, plus
                            // duplicate / delete buttons.
                            ui.horizontal(|ui| {
                                let renaming_this = state
                                    .pending_rename
                                    .as_ref()
                                    .map(|(p, _)| p == &entry.path)
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
                                    let confirm =
                                        ui.button("OK").on_hover_text("Apply rename").clicked()
                                            || (resp.lost_focus()
                                                && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                                    let cancel = ui
                                        .button("✕")
                                        .on_hover_text("Cancel rename")
                                        .clicked();
                                    if confirm {
                                        let new_name = buf.trim().to_owned();
                                        if !new_name.is_empty() && new_name != entry.label {
                                            let mut to = entry
                                                .path
                                                .parent()
                                                .map(Path::to_path_buf)
                                                .unwrap_or_else(|| dir.clone());
                                            to.push(format!("{new_name}.{ext}", ext = entry.ext));
                                            action = Some(BuildsAction::Rename {
                                                from: entry.path.clone(),
                                                to,
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
                                    action = Some(BuildsAction::LoadFile(entry.path.clone()));
                                }

                                ui.weak(format!(".{}", entry.ext));

                                if !renaming_this {
                                    if ui
                                        .small_button("✎")
                                        .on_hover_text("Rename")
                                        .clicked()
                                    {
                                        state.pending_rename =
                                            Some((entry.path.clone(), entry.label.clone()));
                                        state.pending_delete = None;
                                    }
                                    if ui
                                        .small_button("⎘")
                                        .on_hover_text("Duplicate")
                                        .clicked()
                                    {
                                        if let Some(to) = duplicate_target(entry) {
                                            action = Some(BuildsAction::Duplicate {
                                                from: entry.path.clone(),
                                                to,
                                            });
                                            state.loaded = false;
                                        }
                                    }
                                    let pending = state
                                        .pending_delete
                                        .as_ref()
                                        .map(|p| p == &entry.path)
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
                                            .on_hover_text(
                                                "Click again to permanently delete",
                                            )
                                            .clicked()
                                        {
                                            action =
                                                Some(BuildsAction::Delete(entry.path.clone()));
                                            state.pending_delete = None;
                                            state.loaded = false;
                                        }
                                    } else if ui
                                        .small_button("🗑")
                                        .on_hover_text("Delete (click again to confirm)")
                                        .clicked()
                                    {
                                        state.pending_delete = Some(entry.path.clone());
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
        ui.label("Save current build into folder as:");
        ui.add(
            egui::TextEdit::singleline(&mut state.save_buffer)
                .desired_width(220.0)
                .hint_text("filename (no extension)"),
        );
        let trimmed = state.save_buffer.trim();
        let save_enabled = !trimmed.is_empty();
        if ui
            .add_enabled(save_enabled, egui::Button::new("Save here"))
            .clicked()
        {
            let mut path = dir.clone();
            path.push(format!("{trimmed}.mk2"));
            action = Some(BuildsAction::SaveAs(path));
            state.save_buffer.clear();
            state.loaded = false; // refresh after the host writes
        }
    });

    action
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rescan_picks_mk2_and_xml_only() {
        // Use a temp dir under target/ so cargo test cleans it up.
        let dir = std::env::temp_dir().join(format!("pob-ui-builds-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        std::fs::write(dir.join("alpha.mk2"), "MK2|...").unwrap();
        std::fs::write(dir.join("beta.xml"), "<x/>").unwrap();
        std::fs::write(dir.join("readme.txt"), "ignored").unwrap();
        std::fs::write(dir.join("hidden.json"), "ignored").unwrap();

        let entries = rescan(&dir);
        let labels: Vec<_> = entries.iter().map(|e| e.label.as_str()).collect();
        let exts: Vec<_> = entries.iter().map(|e| e.ext.as_str()).collect();
        assert_eq!(labels, vec!["alpha", "beta"]);
        // Extensions are lowercased.
        assert!(exts.contains(&"mk2"));
        assert!(exts.contains(&"xml"));
        assert!(!exts.contains(&"txt"));
        assert!(!exts.contains(&"json"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rescan_alphabetises_case_insensitively() {
        let dir = std::env::temp_dir().join(format!("pob-ui-builds-sort-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        std::fs::write(dir.join("Zeta.mk2"), "").unwrap();
        std::fs::write(dir.join("alpha.mk2"), "").unwrap();
        std::fs::write(dir.join("MIDDLE.mk2"), "").unwrap();

        let entries = rescan(&dir);
        let labels: Vec<_> = entries.iter().map(|e| e.label.clone()).collect();
        assert_eq!(labels, vec!["alpha", "MIDDLE", "Zeta"]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    // Issue #100 (slice 2): one-level subdirectory walk groups
    // builds into categories. Files in the root carry
    // `category = None` (rendered as "Uncategorised"); files inside a
    // subdir carry the directory name as their category. Hidden
    // dirs (starting with `.`) and nested-deeper files don't show.
    #[test]
    fn rescan_walks_one_level_of_subdirs_into_categories() {
        let dir = std::env::temp_dir().join(format!(
            "pob-ui-builds-categories-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // Root-level build.
        std::fs::write(dir.join("scratch.mk2"), "").unwrap();
        // Two named categories with builds inside each.
        std::fs::create_dir_all(dir.join("Levelling")).unwrap();
        std::fs::write(dir.join("Levelling/Phys-RT.mk2"), "").unwrap();
        std::fs::write(dir.join("Levelling/Spell-CI.xml"), "").unwrap();
        std::fs::create_dir_all(dir.join("Bossing")).unwrap();
        std::fs::write(dir.join("Bossing/HC-DD.mk2"), "").unwrap();
        // Hidden dir is ignored.
        std::fs::create_dir_all(dir.join(".cache")).unwrap();
        std::fs::write(dir.join(".cache/old.mk2"), "").unwrap();
        // Nested-deeper builds are ignored (only one level of subdir).
        std::fs::create_dir_all(dir.join("Bossing/sub")).unwrap();
        std::fs::write(dir.join("Bossing/sub/deep.mk2"), "").unwrap();

        let entries = rescan(&dir);
        let categories: Vec<Option<&str>> =
            entries.iter().map(|e| e.category.as_deref()).collect();
        let labels: Vec<&str> = entries.iter().map(|e| e.label.as_str()).collect();

        assert_eq!(
            categories,
            vec![None, Some("Bossing"), Some("Levelling"), Some("Levelling")],
            "Uncategorised first, then categories alphabetised; deeper / hidden dirs skipped"
        );
        assert_eq!(
            labels,
            vec!["scratch", "HC-DD", "Phys-RT", "Spell-CI"],
            "labels stable within each category"
        );
        // The deeper build "deep" must not appear at all.
        assert!(
            !entries.iter().any(|e| e.label == "deep"),
            "files inside nested subdirs must not be flattened up; got {:?}",
            labels
        );
        // The hidden-dir build is ignored too.
        assert!(
            !entries.iter().any(|e| e.label == "old"),
            "files inside hidden dirs must be skipped"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    // Issue #100 (slice 3): duplicate target picks the next free
    // `<name> copy.<ext>` filename, then `<name> copy 2.<ext>` if
    // that one's taken.
    #[test]
    fn duplicate_target_picks_unused_copy_name() {
        let dir = std::env::temp_dir().join(format!(
            "pob-ui-builds-dup-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let path = dir.join("MyBuild.mk2");
        std::fs::write(&path, "").unwrap();
        let entry = BuildEntry {
            label: "MyBuild".into(),
            path: path.clone(),
            ext: "mk2".into(),
            category: None,
        };

        // First call: picks `MyBuild copy.mk2`.
        let target = duplicate_target(&entry).expect("first target");
        assert_eq!(
            target.file_name().and_then(|s| s.to_str()),
            Some("MyBuild copy.mk2"),
            "first duplicate should land at `<name> copy.<ext>`; got {target:?}"
        );

        // Pre-create that file to force the helper to pick a higher
        // suffix on the next call.
        std::fs::write(&target, "").unwrap();
        let target2 = duplicate_target(&entry).expect("second target");
        assert_eq!(
            target2.file_name().and_then(|s| s.to_str()),
            Some("MyBuild copy 2.mk2"),
            "second duplicate should land at `<name> copy 2.<ext>`; got {target2:?}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn builds_dir_is_under_app_namespace() {
        // Whichever platform this test runs on, the path must end with
        // `PathOfBuildingMK2/Builds` so we don't collide with upstream
        // PoB's own folder.
        if let Some(p) = builds_dir() {
            let s = p.to_string_lossy();
            assert!(
                s.ends_with("PathOfBuildingMK2/Builds") || s.ends_with("PathOfBuildingMK2\\Builds"),
                "unexpected builds_dir suffix: {s}"
            );
        }
    }
}
