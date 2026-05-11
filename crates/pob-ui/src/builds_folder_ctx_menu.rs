//! Issue #213 (slice 4): pure state machine for the Builds-tab
//! folder right-click context menu.
//!
//! Slice 3 (#389) shipped the validation + uniqueness helpers in
//! [`crate::builds_folder_ops`]; slice 2 (#380) shipped the
//! [`crate::builds_folder_tree`] renderer. This module is the
//! glue: it owns the popup state for the three folder operations
//! ("Rename folder…", "New subfolder…", "Delete folder") and the
//! pure helpers the renderer + host need to drive them.
//!
//! Why pure? The egui rendering itself isn't tested, but every
//! decision the renderer makes — "is the popup open", "is the
//! save button enabled", "what error should the inline label
//! show", "is the folder empty enough to delete" — flows through
//! these helpers so we can pin the contract under unit tests.
//! Mistakes in this layer would otherwise only surface as
//! interactive bugs.
//!
//! State shape: a single enum variant per popup kind, carrying
//! the path of the folder under operation (slash-joined, matching
//! [`crate::builds_folder_tree::folder_path_key`]) plus any live
//! edit buffer / pre-computed validation error. Only one folder
//! popup can be open at a time — opening a second closes the
//! first, mirroring the build-row rename popup's behaviour.
//!
//! Wiring to disk lives in the host (`lib.rs`), as for all other
//! [`crate::builds_tab::BuildsAction`] variants. The host
//! resolves the slash-joined `folder_path` against the platform
//! builds directory and runs `std::fs::rename` /
//! `std::fs::create_dir` / `std::fs::remove_dir`.

use crate::builds_folder_ops::{validate_folder_name, FolderNameError};
use crate::builds_folder_tree::FolderNode;

/// One of the three folder-context popups currently open. `None`
/// on `BuildsTabState` means no folder popup is open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FolderPopupState {
    /// "Rename folder…" — the user is editing the folder's name
    /// in place. `path` is the slash-joined folder path key
    /// (matches [`crate::builds_folder_tree::folder_path_key`]).
    /// `buffer` is the live text-edit content; `error` is the
    /// last validation error to render inline (recomputed every
    /// frame from `buffer` so it stays in sync with the edit).
    Rename {
        path: String,
        buffer: String,
        error: Option<FolderNameError>,
    },
    /// "New subfolder…" — `parent_path` is the slash-joined path
    /// of the folder the new subdirectory will be created inside
    /// (empty string for "create at the root", though the menu
    /// only attaches to non-root headers in practice).
    NewSubfolder {
        parent_path: String,
        buffer: String,
        error: Option<FolderNameError>,
    },
    /// "Delete folder" — pending confirmation. The actual
    /// emptiness check happens at open-time
    /// ([`start_delete_folder`]) so we don't open a confirm modal
    /// for a folder we know we'd refuse to delete.
    DeleteConfirm { path: String },
}

/// Open-time error from [`start_delete_folder`]. The renderer
/// surfaces this as a transient status message instead of opening
/// a confirm modal that would just refuse on Confirm anyway.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeleteFolderError {
    /// The folder still contains builds (recursively counted).
    /// Carries the count so the message can be specific.
    NotEmpty { build_count: usize },
}

/// Recursively count builds reachable from `node`. Mirrors the
/// renderer's badge counter but lives here because the empty-folder
/// check for delete drives a state transition, not just a label.
#[must_use]
pub fn count_folder_builds(node: &FolderNode) -> usize {
    let mut n = node.builds.len();
    for child in &node.children {
        n += count_folder_builds(child);
    }
    n
}

/// Build the popup state for "Rename folder…". Seeds the buffer
/// with the current folder name so the user edits in place.
///
/// `path` is the full slash-joined folder path; `current_name` is
/// the leaf segment (the part the user actually edits — renaming
/// `Levelling/Marauder` to `Templar` should put `Marauder` in the
/// buffer, not the whole path).
#[must_use]
pub fn start_rename_folder(path: String, current_name: &str) -> FolderPopupState {
    FolderPopupState::Rename {
        path,
        buffer: current_name.to_owned(),
        error: None,
    }
}

/// Build the popup state for "New subfolder…". The buffer starts
/// empty; the Save button is disabled while
/// [`validate_folder_name`] rejects the input.
#[must_use]
pub fn start_new_subfolder(parent_path: String) -> FolderPopupState {
    FolderPopupState::NewSubfolder {
        parent_path,
        buffer: String::new(),
        error: None,
    }
}

/// Build the popup state for "Delete folder", or refuse with
/// [`DeleteFolderError::NotEmpty`] if the folder still contains
/// builds. The renderer uses the `Err` variant to flash a status
/// message and skip opening the confirm modal.
///
/// `path` is the slash-joined folder path; `node` is the matching
/// [`FolderNode`] from the current tree (so we can count builds
/// without re-walking from the root).
pub fn start_delete_folder(
    path: String,
    node: &FolderNode,
) -> Result<FolderPopupState, DeleteFolderError> {
    let count = count_folder_builds(node);
    if count > 0 {
        return Err(DeleteFolderError::NotEmpty { build_count: count });
    }
    Ok(FolderPopupState::DeleteConfirm { path })
}

/// Recompute the validation error for the popup's current buffer.
/// Called every frame the popup is open so the inline error label
/// reflects the live edit. Returns `None` when the buffer would be
/// accepted.
///
/// Distinguishes a "same name" rename (no-op) from a real edit:
/// renaming `Levelling` to `Levelling` shouldn't surface as an
/// error, but the Save button should still be disabled because
/// there's nothing to commit. Callers detect that case with
/// [`is_rename_noop`].
#[must_use]
pub fn validation_error(buffer: &str) -> Option<FolderNameError> {
    validate_folder_name(buffer).err()
}

/// Whether a Rename popup's buffer matches the original name.
/// Used to grey out the Save button so a no-op rename can't
/// accidentally fire (the host's `std::fs::rename(p, p)` would
/// be harmless but the user-visible "Renamed to …" status would
/// be misleading).
#[must_use]
pub fn is_rename_noop(popup: &FolderPopupState) -> bool {
    let FolderPopupState::Rename { path, buffer, .. } = popup else {
        return false;
    };
    let leaf = path.rsplit('/').next().unwrap_or(path);
    buffer.trim() == leaf
}

/// Whether the popup's Save button should be enabled. Combines
/// the validation result with the rename no-op check; the
/// `DeleteConfirm` variant always returns `true` (its single
/// "Confirm delete" button has no buffer to validate).
#[must_use]
pub fn can_commit(popup: &FolderPopupState) -> bool {
    match popup {
        FolderPopupState::Rename { buffer, .. } => {
            validation_error(buffer).is_none() && !is_rename_noop(popup)
        }
        FolderPopupState::NewSubfolder { buffer, .. } => validation_error(buffer).is_none(),
        FolderPopupState::DeleteConfirm { .. } => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builds_folder_tree::build_folder_tree;
    use crate::builds_tab::{BuildEntry, BuildId};
    use std::path::PathBuf;

    fn mk(label: &str, category: Option<&str>) -> BuildEntry {
        BuildEntry {
            label: label.to_owned(),
            id: BuildId::Disk(PathBuf::from(format!("/tmp/{label}.mk2"))),
            ext: "mk2".to_owned(),
            category: category.map(str::to_owned),
        }
    }

    // ----- count_folder_builds -----

    #[test]
    fn count_folder_builds_zero_for_empty_tree() {
        let tree = build_folder_tree(&[]);
        assert_eq!(count_folder_builds(&tree), 0);
    }

    #[test]
    fn count_folder_builds_recurses_through_subfolders() {
        let entries = vec![
            mk("a", Some("Lev")),
            mk("b", Some("Lev/Mar")),
            mk("c", Some("Lev/Mar/Sub")),
        ];
        let tree = build_folder_tree(&entries);
        let lev = tree
            .children
            .iter()
            .find(|c| c.name == "Lev")
            .expect("Lev folder");
        assert_eq!(
            count_folder_builds(lev),
            3,
            "Lev contains a + Mar/{{b, c (in Sub)}}",
        );
    }

    // ----- start_rename_folder -----

    #[test]
    fn start_rename_folder_seeds_buffer_with_current_name() {
        let popup = start_rename_folder("Levelling/Marauder".to_owned(), "Marauder");
        let FolderPopupState::Rename {
            path,
            buffer,
            error,
        } = popup
        else {
            panic!("expected Rename variant");
        };
        assert_eq!(path, "Levelling/Marauder");
        assert_eq!(buffer, "Marauder");
        assert!(error.is_none(), "no error before the user edits");
    }

    // ----- start_new_subfolder -----

    #[test]
    fn start_new_subfolder_starts_with_empty_buffer() {
        let popup = start_new_subfolder("Levelling".to_owned());
        let FolderPopupState::NewSubfolder {
            parent_path,
            buffer,
            error,
        } = popup
        else {
            panic!("expected NewSubfolder variant");
        };
        assert_eq!(parent_path, "Levelling");
        assert!(buffer.is_empty());
        assert!(
            error.is_none(),
            "error stays None until the user types something"
        );
    }

    // ----- start_delete_folder -----

    #[test]
    fn start_delete_folder_allows_empty_folder() {
        // A folder with subfolders but no builds anywhere underneath
        // is still "empty" from the build-count perspective. We treat
        // that as deletable — the host's `std::fs::remove_dir` will
        // refuse non-empty directories on its own and surface the OS
        // error if the user has stale empty subfolders we don't know
        // about. Here we just verify the state-machine path.
        let entries: Vec<BuildEntry> = Vec::new();
        let tree = build_folder_tree(&entries);
        // A node we synthesize so we can call start_delete_folder
        // against an empty FolderNode directly.
        let empty_node = FolderNode {
            name: "Empty".to_owned(),
            children: vec![],
            builds: vec![],
        };
        let _ = tree;
        let result = start_delete_folder("Empty".to_owned(), &empty_node);
        assert!(matches!(
            result,
            Ok(FolderPopupState::DeleteConfirm { path }) if path == "Empty",
        ));
    }

    #[test]
    fn start_delete_folder_refuses_when_builds_exist() {
        let entries = vec![mk("HC", Some("Bossing"))];
        let tree = build_folder_tree(&entries);
        let bossing = tree
            .children
            .iter()
            .find(|c| c.name == "Bossing")
            .expect("Bossing folder");
        let result = start_delete_folder("Bossing".to_owned(), bossing);
        assert_eq!(result, Err(DeleteFolderError::NotEmpty { build_count: 1 }),);
    }

    #[test]
    fn start_delete_folder_counts_nested_builds() {
        // Folder is empty at the top level but has builds three
        // levels down — still must refuse.
        let entries = vec![
            mk("a", Some("Boss/HC/Sirus")),
            mk("b", Some("Boss/HC/Maven")),
        ];
        let tree = build_folder_tree(&entries);
        let boss = tree
            .children
            .iter()
            .find(|c| c.name == "Boss")
            .expect("Boss folder");
        assert_eq!(
            start_delete_folder("Boss".to_owned(), boss),
            Err(DeleteFolderError::NotEmpty { build_count: 2 }),
        );
    }

    // ----- validation_error / can_commit -----

    #[test]
    fn validation_error_passes_through_validate_folder_name() {
        assert!(validation_error("Levelling").is_none());
        assert_eq!(validation_error(""), Some(FolderNameError::Empty));
        assert_eq!(
            validation_error("a/b"),
            Some(FolderNameError::ContainsSeparator),
        );
    }

    #[test]
    fn can_commit_is_false_for_empty_buffer() {
        let popup = FolderPopupState::NewSubfolder {
            parent_path: "Lev".to_owned(),
            buffer: "  ".to_owned(),
            error: None,
        };
        assert!(!can_commit(&popup));
    }

    #[test]
    fn can_commit_is_true_for_valid_new_subfolder_name() {
        let popup = FolderPopupState::NewSubfolder {
            parent_path: "Lev".to_owned(),
            buffer: "Marauder".to_owned(),
            error: None,
        };
        assert!(can_commit(&popup));
    }

    #[test]
    fn can_commit_is_false_for_rename_to_same_name() {
        // No-op rename: the buffer still equals the leaf segment of
        // the path. Save should be disabled so the user can't fire a
        // misleading "Renamed to Marauder" status.
        let popup = FolderPopupState::Rename {
            path: "Levelling/Marauder".to_owned(),
            buffer: "Marauder".to_owned(),
            error: None,
        };
        assert!(!can_commit(&popup));
        assert!(is_rename_noop(&popup));
    }

    #[test]
    fn can_commit_is_true_for_rename_to_a_different_valid_name() {
        let popup = FolderPopupState::Rename {
            path: "Levelling/Marauder".to_owned(),
            buffer: "Templar".to_owned(),
            error: None,
        };
        assert!(can_commit(&popup));
        assert!(!is_rename_noop(&popup));
    }

    #[test]
    fn can_commit_is_false_for_rename_with_invalid_name() {
        let popup = FolderPopupState::Rename {
            path: "Levelling".to_owned(),
            buffer: "bad/name".to_owned(),
            error: None,
        };
        assert!(!can_commit(&popup));
    }

    #[test]
    fn can_commit_is_always_true_for_delete_confirm() {
        // The Confirm-delete button has no buffer to validate.
        let popup = FolderPopupState::DeleteConfirm {
            path: "Lev".to_owned(),
        };
        assert!(can_commit(&popup));
    }

    #[test]
    fn rename_noop_check_handles_root_level_path() {
        // A folder directly off the root has no `/` in its path.
        // The leaf calc must still work.
        let popup = FolderPopupState::Rename {
            path: "Bossing".to_owned(),
            buffer: "Bossing".to_owned(),
            error: None,
        };
        assert!(is_rename_noop(&popup));
    }

    #[test]
    fn rename_noop_check_trims_buffer_whitespace() {
        // `validate_folder_name` trims; the no-op check should match
        // so the user can't unstick a no-op by adding a trailing
        // space.
        let popup = FolderPopupState::Rename {
            path: "Lev".to_owned(),
            buffer: "  Lev  ".to_owned(),
            error: None,
        };
        assert!(is_rename_noop(&popup));
        assert!(!can_commit(&popup));
    }
}
