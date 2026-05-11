//! Desktop builds-folder helpers — resolve the platform-specific
//! builds directory, rescan it, and pick non-clashing duplicate
//! filenames. Migrated out of `builds_tab.rs` so the UI module stays
//! storage-agnostic and the wasm backend (see [`build_store_wasm`])
//! can plug into the same [`BuildEntry`] shape.

use std::path::{Path, PathBuf};

use crate::builds_tab::{BuildEntry, BuildId};

/// Resolve the platform-specific builds directory. Returns `None` if
/// the environment doesn't carry the relevant home/appdata variable
/// (test runners, daemons running as root, etc.).
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
/// doesn't exist (so the user has somewhere to save into). Walks one
/// level of subdirectories — files in nested subdirs and files inside
/// hidden dirs are dropped. Resulting entries are category-then-label
/// sorted with `Uncategorised` first.
pub fn rescan(dir: &Path) -> Vec<BuildEntry> {
    let _ = std::fs::create_dir_all(dir);
    let mut out: Vec<BuildEntry> = Vec::new();
    if let Ok(read) = std::fs::read_dir(dir) {
        for entry in read.filter_map(|e| e.ok()) {
            let path = entry.path();
            let file_type = entry.file_type().ok();
            if file_type.map(|t| t.is_dir()).unwrap_or(false) {
                let category = path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .filter(|s| !s.starts_with('.'))
                    .map(str::to_owned);
                let Some(category) = category else { continue };
                if let Ok(child) = std::fs::read_dir(&path) {
                    for f in child.filter_map(|e| e.ok()) {
                        if let Some(be) = build_entry_from_path(f.path(), Some(category.clone())) {
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
        let ca = a.category.as_deref().unwrap_or("");
        let cb = b.category.as_deref().unwrap_or("");
        let by_cat = match (a.category.is_some(), b.category.is_some()) {
            (false, true) => std::cmp::Ordering::Less,
            (true, false) => std::cmp::Ordering::Greater,
            _ => ca.to_ascii_lowercase().cmp(&cb.to_ascii_lowercase()),
        };
        by_cat.then_with(|| {
            a.label
                .to_ascii_lowercase()
                .cmp(&b.label.to_ascii_lowercase())
        })
    });
    out
}

/// Issue #213: compute the destination path for a move-to-folder
/// operation. `from` is the current path of the build file; `root` is
/// the configured builds directory; `target_folder` is the slash-joined
/// folder path the build should land in (`None` and `Some("")` both
/// mean "the root").
///
/// Returns `None` when the helper can't form a meaningful target — the
/// usual cause is a `from` path without a file name (i.e. someone
/// passed a directory). Empty / whitespace-only segments inside
/// `target_folder` are stripped so callers can construct paths
/// loosely; the resulting `PathBuf` always uses native separators.
///
/// The helper is pure: it does **not** touch the filesystem, doesn't
/// create the target directory, and doesn't check for conflicts. The
/// caller (the desktop action handler) creates parents and surfaces
/// rename failures.
#[must_use]
pub fn move_to_folder_target(
    from: &Path,
    root: &Path,
    target_folder: Option<&str>,
) -> Option<PathBuf> {
    let file_name = from.file_name()?;
    let mut target = root.to_path_buf();
    if let Some(folder_path) = target_folder {
        for segment in folder_path.split('/').map(str::trim) {
            if !segment.is_empty() {
                target.push(segment);
            }
        }
    }
    target.push(file_name);
    Some(target)
}

/// Pick an unused destination path for a duplicate of `entry`. Tries
/// `<name> copy.<ext>` first, then `<name> copy 2.<ext>`, etc. Returns
/// `None` if `entry.id` isn't a `Disk` variant or has no parent
/// directory (shouldn't happen for a real build file).
pub fn duplicate_target(entry: &BuildEntry) -> Option<PathBuf> {
    let BuildId::Disk(path) = &entry.id else {
        return None;
    };
    let parent = path.parent()?;
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
        id: BuildId::Disk(path),
        ext,
        category,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rescan_picks_mk2_and_xml_only() {
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

    #[test]
    fn rescan_walks_one_level_of_subdirs_into_categories() {
        let dir =
            std::env::temp_dir().join(format!("pob-ui-builds-categories-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        std::fs::write(dir.join("scratch.mk2"), "").unwrap();
        std::fs::create_dir_all(dir.join("Levelling")).unwrap();
        std::fs::write(dir.join("Levelling/Phys-RT.mk2"), "").unwrap();
        std::fs::write(dir.join("Levelling/Spell-CI.xml"), "").unwrap();
        std::fs::create_dir_all(dir.join("Bossing")).unwrap();
        std::fs::write(dir.join("Bossing/HC-DD.mk2"), "").unwrap();
        std::fs::create_dir_all(dir.join(".cache")).unwrap();
        std::fs::write(dir.join(".cache/old.mk2"), "").unwrap();
        std::fs::create_dir_all(dir.join("Bossing/sub")).unwrap();
        std::fs::write(dir.join("Bossing/sub/deep.mk2"), "").unwrap();

        let entries = rescan(&dir);
        let categories: Vec<Option<&str>> = entries.iter().map(|e| e.category.as_deref()).collect();
        let labels: Vec<&str> = entries.iter().map(|e| e.label.as_str()).collect();

        assert_eq!(
            categories,
            vec![None, Some("Bossing"), Some("Levelling"), Some("Levelling")],
        );
        assert_eq!(labels, vec!["scratch", "HC-DD", "Phys-RT", "Spell-CI"]);
        assert!(!entries.iter().any(|e| e.label == "deep"));
        assert!(!entries.iter().any(|e| e.label == "old"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn duplicate_target_picks_unused_copy_name() {
        let dir = std::env::temp_dir().join(format!("pob-ui-builds-dup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let path = dir.join("MyBuild.mk2");
        std::fs::write(&path, "").unwrap();
        let entry = BuildEntry {
            label: "MyBuild".into(),
            id: BuildId::Disk(path.clone()),
            ext: "mk2".into(),
            category: None,
        };

        let target = duplicate_target(&entry).expect("first target");
        assert_eq!(
            target.file_name().and_then(|s| s.to_str()),
            Some("MyBuild copy.mk2"),
        );

        std::fs::write(&target, "").unwrap();
        let target2 = duplicate_target(&entry).expect("second target");
        assert_eq!(
            target2.file_name().and_then(|s| s.to_str()),
            Some("MyBuild copy 2.mk2"),
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn move_to_folder_moves_build_into_subdir() {
        // Issue #213: a build sitting at the builds-dir root moves
        // into a subfolder. The helper joins the target folder onto
        // the root and re-appends the file name.
        let root = Path::new("/builds");
        let from = Path::new("/builds/MyBuild.mk2");
        let target = move_to_folder_target(from, root, Some("Bossing")).expect("target");
        assert_eq!(target, PathBuf::from("/builds/Bossing/MyBuild.mk2"));
    }

    #[test]
    fn move_to_folder_handles_nested_target() {
        // Slash-joined folder path lands as nested segments.
        let root = Path::new("/builds");
        let from = Path::new("/builds/MyBuild.mk2");
        let target =
            move_to_folder_target(from, root, Some("Bossing/Uber")).expect("nested target");
        assert_eq!(target, PathBuf::from("/builds/Bossing/Uber/MyBuild.mk2"));
    }

    #[test]
    fn move_to_folder_lands_at_root_for_none_or_empty() {
        // Both `None` and `Some("")` mean "the root". Mirrors PoB
        // where the root is `category = nil` and an empty category
        // string means the same thing.
        let root = Path::new("/builds");
        let from = Path::new("/builds/Levelling/MyBuild.mk2");
        let to_root = move_to_folder_target(from, root, None).expect("root via None");
        assert_eq!(to_root, PathBuf::from("/builds/MyBuild.mk2"));
        let to_root2 = move_to_folder_target(from, root, Some("")).expect("root via empty");
        assert_eq!(to_root2, PathBuf::from("/builds/MyBuild.mk2"));
    }

    #[test]
    fn move_to_folder_strips_whitespace_and_empty_segments() {
        // Defensive: a hand-built folder path like `"Bossing//Uber/"`
        // shouldn't produce an empty path component or trailing
        // separator — the helper trims and skips empties.
        let root = Path::new("/builds");
        let from = Path::new("/builds/MyBuild.mk2");
        let target =
            move_to_folder_target(from, root, Some("  Bossing  /  / Uber  / ")).expect("target");
        assert_eq!(target, PathBuf::from("/builds/Bossing/Uber/MyBuild.mk2"));
    }

    #[test]
    fn move_to_folder_returns_none_for_root_only_path() {
        // Passing a path with no file-name component (e.g. the
        // filesystem root) is a caller bug — the helper bails rather
        // than concocting a meaningless target. Real call sites
        // always pass a build-file path with a name attached.
        let root = Path::new("/builds");
        let from = Path::new("/");
        assert!(move_to_folder_target(from, root, Some("Bossing")).is_none());
    }

    #[test]
    fn move_to_folder_preserves_filename_extension() {
        // Multi-dot filenames and unusual extensions (xml round-tripped
        // from PoB) must survive intact — the helper relies on
        // `Path::file_name`, not string manipulation.
        let root = Path::new("/builds");
        let from = Path::new("/builds/My Build name.with.dots.xml");
        let target = move_to_folder_target(from, root, Some("Saved")).expect("target");
        assert_eq!(
            target,
            PathBuf::from("/builds/Saved/My Build name.with.dots.xml"),
        );
    }

    #[test]
    fn builds_dir_is_under_app_namespace() {
        if let Some(p) = builds_dir() {
            let s = p.to_string_lossy();
            assert!(
                s.ends_with("PathOfBuildingMK2/Builds") || s.ends_with("PathOfBuildingMK2\\Builds"),
                "unexpected builds_dir suffix: {s}"
            );
        }
    }
}
