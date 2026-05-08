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
}

#[derive(Debug, Clone)]
pub struct BuildEntry {
    /// File-stem display label (e.g. "MyBuild").
    pub label: String,
    /// Absolute path to the file on disk.
    pub path: PathBuf,
    /// File extension lowercased (`mk2` / `xml`).
    pub ext: String,
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
/// doesn't exist (so the user has somewhere to save into).
fn rescan(dir: &Path) -> Vec<BuildEntry> {
    let _ = std::fs::create_dir_all(dir);
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<BuildEntry> = read
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let path = e.path();
            let ext = path
                .extension()
                .and_then(|s| s.to_str())
                .map(|s| s.to_ascii_lowercase())?;
            if ext != "mk2" && ext != "xml" {
                return None;
            }
            let label = path.file_stem().and_then(|s| s.to_str())?.to_owned();
            Some(BuildEntry { label, path, ext })
        })
        .collect();
    out.sort_by(|a, b| a.label.to_ascii_lowercase().cmp(&b.label.to_ascii_lowercase()));
    out
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
    ui.separator();

    if state.entries.is_empty() {
        ui.label(
            "No builds saved yet. Save the current build into this folder via \
             \"Save here\" below — or use File → Save As to pick any path manually.",
        );
    } else {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .max_height(360.0)
            .show(ui, |ui| {
                egui::Grid::new("builds_grid")
                    .num_columns(2)
                    .striped(true)
                    .show(ui, |ui| {
                        for entry in &state.entries {
                            if ui
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
                            ui.end_row();
                        }
                    });
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
        let dir = std::env::temp_dir().join(format!(
            "pob-ui-builds-test-{}",
            std::process::id()
        ));
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
        let dir = std::env::temp_dir().join(format!(
            "pob-ui-builds-sort-{}",
            std::process::id()
        ));
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

    #[test]
    fn builds_dir_is_under_app_namespace() {
        // Whichever platform this test runs on, the path must end with
        // `PathOfBuildingMK2/Builds` so we don't collide with upstream
        // PoB's own folder.
        if let Some(p) = builds_dir() {
            let s = p.to_string_lossy();
            assert!(
                s.ends_with("PathOfBuildingMK2/Builds")
                    || s.ends_with("PathOfBuildingMK2\\Builds"),
                "unexpected builds_dir suffix: {s}"
            );
        }
    }
}
