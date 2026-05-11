//! Shared-item store for the Items tab (issue #209).
//!
//! Mirrors PoB's `SharedItemListControl.lua` in spirit: lets the user save
//! any equipped item under a label and pull it back into a slot later.
//! The store persists as a JSON file in the platform data dir on native
//! targets so saves survive across app restarts (the issue's last
//! acceptance criterion). On wasm we keep the list in memory for the
//! current session — a future slice can wire it into IndexedDB the same
//! way [`build_store_wasm`] does for builds.
//!
//! The store deliberately keeps no foreign keys to a particular build —
//! shared items are user-global so they round-trip across builds.

use std::path::PathBuf;

use pob_data::Item;
use serde::{Deserialize, Serialize};

/// One saved entry in the store. `label` is the user-facing name on the
/// browse panel row; `item` is the full item snapshot (mod lines,
/// rarity, sockets, raw paste — everything we need to round-trip it
/// back into a slot byte-for-byte).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedItem {
    pub label: String,
    pub item: Item,
}

/// In-memory list of saved shared items. `dirty` tracks unsaved
/// changes so the host can flush to disk at the right moment without
/// re-writing the file on every frame.
#[derive(Debug, Default, Clone)]
pub struct SharedItemStore {
    pub items: Vec<SharedItem>,
    /// `true` after any mutating call — `flush` clears it on a successful
    /// write. The host calls `flush` after handling browse-panel actions.
    pub dirty: bool,
}

impl SharedItemStore {
    /// New, empty store. Used by tests and the wasm boot path.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an item. The label is auto-deduplicated by appending
    /// ` (n)` so the user can hit "Save to shared" twice without
    /// silently overwriting the previous save (mirrors the
    /// `duplicate_target` pattern used by the builds folder).
    pub fn add(&mut self, label: impl Into<String>, item: Item) -> &SharedItem {
        let mut label = label.into();
        if label.trim().is_empty() {
            // Fall back to the item's own name / base so the user
            // always gets something readable, even on a blank label.
            label = if !item.name.is_empty() {
                item.name.clone()
            } else {
                item.base_name.clone()
            };
            if label.trim().is_empty() {
                label = "Untitled".to_owned();
            }
        }
        let unique = self.unique_label(&label);
        self.items.push(SharedItem {
            label: unique,
            item,
        });
        self.dirty = true;
        self.items.last().expect("just pushed")
    }

    /// Issue #211 (slice 2): duplicate the entry at `index`, with the
    /// label run through [`Self::unique_label`] so the new row stays
    /// distinguishable in the dropdown. Returns the new index, or
    /// `None` when `index` is out of range. Wired into the
    /// shared-items right-click context menu's "Clone" entry.
    pub fn clone_at(&mut self, index: usize) -> Option<usize> {
        let source = self.items.get(index)?;
        let new_label = self.unique_label(&source.label);
        let new_item = source.item.clone();
        self.items.push(SharedItem {
            label: new_label,
            item: new_item,
        });
        self.dirty = true;
        Some(self.items.len() - 1)
    }

    /// Issue #211 (slice 3): relabel the entry at `index`. The new
    /// label is run through [`Self::unique_label`] so renaming onto a
    /// collision (e.g. "Boots" when another "Boots" already exists)
    /// produces a `Label (2)` suffix — matching the dedup semantics of
    /// [`Self::add`] and [`Self::clone_at`]. Empty / whitespace-only
    /// labels are rejected because a blank row is unreadable in the
    /// dropdown; the caller's inline-edit popup also short-circuits but
    /// pinning it here keeps the data-layer contract honest for any
    /// future caller. Returns `true` on success, `false` if `index` is
    /// out of range or the label was rejected.
    pub fn rename(&mut self, index: usize, new_label: &str) -> bool {
        if index >= self.items.len() {
            return false;
        }
        let trimmed = new_label.trim();
        if trimmed.is_empty() {
            return false;
        }
        // No-op when the user re-confirms the existing label —
        // otherwise `unique_label` would see the row itself as a
        // collision and append "(2)".
        if self.items[index].label == trimmed {
            self.dirty = true;
            return true;
        }
        let unique = self.unique_label(trimmed);
        self.items[index].label = unique;
        self.dirty = true;
        true
    }

    /// Remove the entry at `index` (no-op if out of range). Returns
    /// `true` if a row was removed.
    pub fn remove(&mut self, index: usize) -> bool {
        if index >= self.items.len() {
            return false;
        }
        self.items.remove(index);
        self.dirty = true;
        true
    }

    /// Clone the item at `index` so the caller can equip it into a slot.
    /// Public API kept for future browse-panel callers; the current path
    /// indexes `items` directly inside the row-render loop.
    #[allow(dead_code)]
    #[must_use]
    pub fn get(&self, index: usize) -> Option<&SharedItem> {
        self.items.get(index)
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Replace the contents wholesale. Used by the disk-load path so a
    /// successful read overwrites the in-memory state without touching
    /// the dirty flag (we just synced from disk — nothing to flush).
    pub fn set_loaded(&mut self, items: Vec<SharedItem>) {
        self.items = items;
        self.dirty = false;
    }

    /// JSON serialisation used by the disk-flush path. Pretty-printed
    /// so users who poke at the file see something readable.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(&self.items)
    }

    /// JSON deserialisation. Returns an empty store on a missing /
    /// empty payload so a fresh install boots clean.
    pub fn from_json(json: &str) -> Result<Vec<SharedItem>, serde_json::Error> {
        let trimmed = json.trim();
        if trimmed.is_empty() {
            return Ok(Vec::new());
        }
        serde_json::from_str(trimmed)
    }

    fn unique_label(&self, candidate: &str) -> String {
        if !self.items.iter().any(|s| s.label == candidate) {
            return candidate.to_owned();
        }
        for n in 2..1000 {
            let trial = format!("{candidate} ({n})");
            if !self.items.iter().any(|s| s.label == trial) {
                return trial;
            }
        }
        // Fall-through: 1000 dupes — extremely unlikely. Just append
        // a deterministic suffix from the current length so the call
        // never panics.
        format!("{candidate} ({})", self.items.len() + 1)
    }
}

/// Resolve the on-disk shared-items file. Mirrors the layout
/// `build_store_disk::builds_dir` uses — under the same
/// `PathOfBuildingMK2` namespace so backup/migration tools see one
/// app folder.
#[cfg(not(target_arch = "wasm32"))]
#[must_use]
pub fn shared_items_path() -> Option<PathBuf> {
    let mut p = data_dir()?;
    p.push("shared_items.json");
    Some(p)
}

/// Platform-specific app data dir. Returns `None` if the relevant
/// HOME / APPDATA variable is missing (CI sandboxes etc.); callers
/// fall back to in-memory only.
#[cfg(not(target_arch = "wasm32"))]
fn data_dir() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var_os("HOME")?;
        let mut p = PathBuf::from(home);
        p.push("Library");
        p.push("Application Support");
        p.push("PathOfBuildingMK2");
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
        Some(p)
    }
    #[cfg(target_os = "windows")]
    {
        let appdata = std::env::var_os("APPDATA")?;
        let mut p = PathBuf::from(appdata);
        p.push("PathOfBuildingMK2");
        Some(p)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        None
    }
}

/// Read the on-disk shared items, returning an empty list if the file
/// is missing / unreadable. Disk corruption is logged through `tracing`
/// but never bubbles up — a corrupt file shouldn't take the Items tab
/// down with it.
#[cfg(not(target_arch = "wasm32"))]
#[must_use]
pub fn load_from_disk() -> Vec<SharedItem> {
    let Some(path) = shared_items_path() else {
        return Vec::new();
    };
    let Ok(json) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    match SharedItemStore::from_json(&json) {
        Ok(items) => items,
        Err(e) => {
            tracing::warn!(
                "Couldn't parse shared items at {}: {e} — starting empty",
                path.display(),
            );
            Vec::new()
        }
    }
}

/// Write the store to disk, creating the parent dir on demand.
/// Returns `Err` only on actual I/O failure (so the host can surface a
/// status toast); a missing data dir is treated as success-without-disk.
#[cfg(not(target_arch = "wasm32"))]
pub fn save_to_disk(store: &SharedItemStore) -> std::io::Result<()> {
    let Some(path) = shared_items_path() else {
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = store
        .to_json()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(&path, json)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ahash::HashSet;
    use pob_data::{ModLine, ModSection, Rarity};

    fn make_item(name: &str, base: &str) -> Item {
        Item {
            name: name.to_owned(),
            base_name: base.to_owned(),
            rarity: Rarity::Rare,
            item_level: 84,
            quality: 20,
            tags: HashSet::default(),
            mod_lines: vec![ModLine {
                line: "+50 to maximum Life".into(),
                section: ModSection::Explicit,
            }],
            sockets: String::new(),
            raw: String::new(),
            corrupted: false,
            mirrored: false,
        }
    }

    #[test]
    fn add_marks_dirty_and_appends() {
        let mut store = SharedItemStore::new();
        assert!(!store.dirty);
        store.add("My Belt", make_item("Stormgut", "Heavy Belt"));
        assert!(store.dirty);
        assert_eq!(store.len(), 1);
        assert_eq!(store.items[0].label, "My Belt");
    }

    #[test]
    fn add_dedupes_on_duplicate_label() {
        let mut store = SharedItemStore::new();
        let item = make_item("Headhunter", "Leather Belt");
        store.add("Belt", item.clone());
        store.add("Belt", item.clone());
        store.add("Belt", item);
        let labels: Vec<&str> = store.items.iter().map(|s| s.label.as_str()).collect();
        assert_eq!(labels, vec!["Belt", "Belt (2)", "Belt (3)"]);
    }

    #[test]
    fn add_with_blank_label_falls_back_to_item_name() {
        let mut store = SharedItemStore::new();
        store.add("", make_item("Headhunter", "Leather Belt"));
        store.add("   ", make_item("", "Onyx Amulet"));
        store.add("\t", make_item("", "")); // truly empty
        let labels: Vec<&str> = store.items.iter().map(|s| s.label.as_str()).collect();
        assert_eq!(labels, vec!["Headhunter", "Onyx Amulet", "Untitled"]);
    }

    #[test]
    fn clone_at_appends_uniquely_labelled_copy_and_marks_dirty() {
        // Issue #211 (slice 2): the shared-items right-click context
        // menu offers a "Clone" entry so the user can branch a saved
        // item before tweaking the copy in their build. The clone
        // must carry a unique label (otherwise the existing
        // `unique_label` dedup logic would have to be re-run by the
        // caller) and the in-memory store must mark itself dirty so
        // the per-frame disk-flush picks it up. Returning the new
        // index lets the UI scroll-to / highlight the clone.
        let mut store = SharedItemStore::new();
        store.add("Boots", make_item("Kaom's Roots", "Titan Greaves"));
        store.dirty = false;
        let new_idx = store.clone_at(0).expect("idx 0 is in range");
        assert!(store.dirty);
        assert_eq!(store.len(), 2);
        // The cloned item shares the underlying item but lives under
        // a `Label (2)` style suffix so the dropdown stays unambiguous.
        assert_eq!(store.items[new_idx].label, "Boots (2)");
        assert_eq!(
            store.items[new_idx].item.base_name,
            store.items[0].item.base_name
        );
    }

    #[test]
    fn clone_at_out_of_range_returns_none_and_does_not_mark_dirty() {
        let mut store = SharedItemStore::new();
        store.add("Boots", make_item("Kaom's Roots", "Titan Greaves"));
        store.dirty = false;
        assert!(store.clone_at(99).is_none());
        assert!(!store.dirty);
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn rename_happy_path_updates_label_and_marks_dirty() {
        // Issue #211 (slice 3): the shared-items right-click context
        // menu offers an inline "Rename" action so users can fix typos
        // or relabel imports without re-saving. The data-layer helper
        // must update the label in place and flip `dirty` so the
        // per-frame disk-flush picks the change up.
        let mut store = SharedItemStore::new();
        store.add("Boots", make_item("Kaom's Roots", "Titan Greaves"));
        store.dirty = false;
        assert!(store.rename(0, "My favourite boots"));
        assert!(store.dirty);
        assert_eq!(store.items[0].label, "My favourite boots");
    }

    #[test]
    fn rename_rejects_empty_or_whitespace_label() {
        // An empty / whitespace-only label is meaningless on the row
        // (it would render as a blank line) so the rename must refuse
        // and leave the store unchanged. The UI also short-circuits on
        // this, but pinning it at the data layer means the contract
        // holds for any future caller.
        let mut store = SharedItemStore::new();
        store.add("Boots", make_item("Kaom's Roots", "Titan Greaves"));
        store.dirty = false;
        assert!(!store.rename(0, ""));
        assert!(!store.rename(0, "   "));
        assert!(!store.rename(0, "\t\n"));
        assert!(!store.dirty);
        assert_eq!(store.items[0].label, "Boots");
    }

    #[test]
    fn rename_out_of_range_returns_false() {
        let mut store = SharedItemStore::new();
        store.add("Boots", make_item("Kaom's Roots", "Titan Greaves"));
        store.dirty = false;
        assert!(!store.rename(99, "Anything"));
        assert!(!store.dirty);
        assert_eq!(store.items[0].label, "Boots");
    }

    #[test]
    fn rename_to_existing_label_routes_through_unique_label() {
        // Renaming "Boots" to "Helm" when a "Helm" already exists
        // should land as "Helm (2)" so the dropdown stays unambiguous.
        // Mirrors the dedup behaviour of `add` / `clone_at`.
        let mut store = SharedItemStore::new();
        store.add("Helm", make_item("Devoto's Devotion", "Nightmare Bascinet"));
        store.add("Boots", make_item("Kaom's Roots", "Titan Greaves"));
        store.dirty = false;
        assert!(store.rename(1, "Helm"));
        assert_eq!(store.items[1].label, "Helm (2)");
        assert!(store.dirty);
    }

    #[test]
    fn rename_to_same_label_is_a_noop_but_still_succeeds() {
        // Renaming a row to its current label shouldn't trip the
        // dedup path (no "Boots (2)") — the candidate matches its own
        // entry, not a different one. We still return `true` because
        // the user's intent (save this label) succeeded.
        let mut store = SharedItemStore::new();
        store.add("Boots", make_item("Kaom's Roots", "Titan Greaves"));
        store.dirty = false;
        assert!(store.rename(0, "Boots"));
        assert_eq!(store.items[0].label, "Boots");
    }

    #[test]
    fn remove_clamps_and_marks_dirty() {
        let mut store = SharedItemStore::new();
        store.add("a", make_item("a", "x"));
        store.add("b", make_item("b", "x"));
        store.dirty = false;
        assert!(!store.remove(99));
        assert!(!store.dirty);
        assert!(store.remove(0));
        assert!(store.dirty);
        assert_eq!(store.items[0].label, "b");
    }

    #[test]
    fn round_trip_through_json_preserves_items() {
        let mut store = SharedItemStore::new();
        store.add("Rare belt", make_item("Stormgut", "Heavy Belt"));
        store.add("Boots", make_item("Kaom's Roots", "Titan Greaves"));
        let json = store.to_json().unwrap();
        let loaded = SharedItemStore::from_json(&json).unwrap();
        // `Item` doesn't implement `PartialEq` (it owns a HashSet
        // of tags whose Eq isn't derived), so compare on the
        // structurally-stable fields. That's enough to gate the
        // round-trip — any breakage in mod-line / rarity /
        // base-name serialisation would still surface here.
        assert_eq!(loaded.len(), store.len());
        for (a, b) in loaded.iter().zip(store.items.iter()) {
            assert_eq!(a.label, b.label);
            assert_eq!(a.item.name, b.item.name);
            assert_eq!(a.item.base_name, b.item.base_name);
            assert_eq!(a.item.rarity, b.item.rarity);
            assert_eq!(a.item.mod_lines.len(), b.item.mod_lines.len());
        }
    }

    #[test]
    fn empty_or_blank_json_loads_as_empty() {
        assert!(SharedItemStore::from_json("").unwrap().is_empty());
        assert!(SharedItemStore::from_json("   \n\t").unwrap().is_empty());
        assert!(SharedItemStore::from_json("[]").unwrap().is_empty());
    }

    #[test]
    fn set_loaded_clears_dirty_flag() {
        let mut store = SharedItemStore::new();
        store.add("a", make_item("a", "x"));
        assert!(store.dirty);
        store.set_loaded(vec![SharedItem {
            label: "from disk".into(),
            item: make_item("d", "Y"),
        }]);
        assert!(!store.dirty);
        assert_eq!(store.len(), 1);
        assert_eq!(store.items[0].label, "from disk");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn save_then_load_round_trips_through_disk() {
        // Drop a temp HOME so the store file lands somewhere we can
        // clean up. We restore HOME afterwards even on assertion
        // failure to avoid poisoning later tests in the same process.
        let prev_home = std::env::var_os("HOME");
        let prev_xdg = std::env::var_os("XDG_DATA_HOME");
        let prev_appdata = std::env::var_os("APPDATA");
        let dir = std::env::temp_dir().join(format!(
            "pob-ui-shared-test-{}-{:p}",
            std::process::id(),
            &prev_home as *const _
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // Each platform reads a different env var. Set them all so the
        // test doesn't go fishing in the host's real data dir.
        std::env::set_var("HOME", &dir);
        std::env::set_var("XDG_DATA_HOME", &dir);
        std::env::set_var("APPDATA", &dir);

        let outcome = std::panic::catch_unwind(|| {
            let mut store = SharedItemStore::new();
            store.add("Rare belt", make_item("Stormgut", "Heavy Belt"));
            save_to_disk(&store).expect("save");
            let loaded = load_from_disk();
            assert_eq!(loaded.len(), 1);
            assert_eq!(loaded[0].label, "Rare belt");
            assert_eq!(loaded[0].item.base_name, "Heavy Belt");
        });

        // Restore env vars.
        match prev_home {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
        match prev_xdg {
            Some(v) => std::env::set_var("XDG_DATA_HOME", v),
            None => std::env::remove_var("XDG_DATA_HOME"),
        }
        match prev_appdata {
            Some(v) => std::env::set_var("APPDATA", v),
            None => std::env::remove_var("APPDATA"),
        }
        let _ = std::fs::remove_dir_all(&dir);
        if let Err(e) = outcome {
            std::panic::resume_unwind(e);
        }
    }
}
