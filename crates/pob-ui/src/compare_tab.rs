//! Compare tab — snapshot the current build and surface stat-by-stat
//! deltas against the live one. PoB exposes the same as `compareTab`
//! in `Modules/Build.lua`. Two snapshot sources:
//!
//! * **Snapshot current build** — captures the live character + output
//!   in memory.
//! * **Load comparison from file** — picks a `.mk2` / `.xml` / share-
//!   code file, imports it, and the host runs `compute_full` against
//!   it to populate the snapshot Output.
//!
//! Each frame, every change to the live build shows up here as a
//! coloured delta vs. the snapshot's stats.

use std::path::PathBuf;

use eframe::egui;
use pob_data::{Item, ItemSet, NodeId, PassiveTree, Slot};
use pob_engine::{Character, Output};

use crate::tree_diff::tree_diff;

#[derive(Debug, Clone, Default)]
pub struct CompareTabState {
    pub snapshot: Option<Snapshot>,
    pub filter: String,
    pub hide_zero_delta: bool,
    /// Issue #223: sort mode for the diff table. PoB exposes the same
    /// three orderings on `CompareTab`: alphabetical by key (default),
    /// by absolute delta descending, by percent delta descending.
    pub sort_mode: CompareSortMode,
    /// Issue #223 follow-up: in-flight rename buffer. `Some(buf)`
    /// renders an inline `TextEdit` in place of the snapshot label;
    /// `None` is the resting state. Commit on Enter writes
    /// [`sanitise_snapshot_label`]'s output back to
    /// `snapshot.label`; Escape / blur clears the buffer without
    /// applying.
    pub pending_relabel: Option<String>,
    /// Issue #223 follow-up: which serialisation the "Copy as ..."
    /// button emits. Defaults to Markdown — the pre-#223 behaviour.
    pub export_format: CompareExportFormat,
}

/// Issue #223: how to order the diff rows. PoB sorts by absolute delta
/// by default (biggest changes first), but the alphabetical fallback
/// is useful for hunting a specific stat in a long list and the
/// percent view normalises across magnitudes (a +50 to a 1k stat reads
/// differently than +50 to a 50 stat — percent collapses that).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CompareSortMode {
    /// Alphabetical by output key. The pre-#223 default; kept first so
    /// the existing fmt-only behaviour is preserved when the field
    /// defaults.
    #[default]
    Key,
    /// Descending by absolute delta — biggest changes first, regardless
    /// of sign.
    AbsDelta,
    /// Descending by percent delta — biggest *relative* changes first.
    /// Rows where the snapshot value is zero (so percent is undefined)
    /// fall to the bottom so they don't poison the ordering with
    /// `inf` / sentinel values.
    PercentDelta,
}

#[derive(Debug, Clone)]
pub struct Snapshot {
    /// Character at snapshot time. Held so a future UI can describe
    /// what's being compared (e.g. "vs L92 Witch Arc on 2026-05-08").
    /// Clippy flags it as unread today — `#[allow]` keeps it compiling
    /// without losing the field.
    #[allow(dead_code)]
    pub character: Character,
    /// Output of `compute_full` against the snapshot character. The
    /// compare panel diffs the live `Output` against this one.
    pub output: Output,
    /// Human-readable label captured at snapshot time.
    pub label: String,
    /// Issue #223: file the snapshot was loaded from, if any. Drives
    /// the "Re-import current" button — clicking it re-reads the file
    /// off disk and re-runs `compute_full`, so a user can iterate
    /// against a saved build elsewhere and pull updates in without
    /// re-traversing the file picker. `None` for in-memory snapshots
    /// (the "Snapshot current build" button captures the live state
    /// directly with no source on disk).
    pub source_path: Option<PathBuf>,
}

/// Action the host should take after the user interacted with the
/// Compare tab — currently only "load a comparison build from disk."
/// The host runs the file-pick dialog, imports the build, and runs
/// `compute_full` against it to populate the snapshot Output, then
/// writes the result back via `state.snapshot = Some(...)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompareAction {
    LoadFromFile,
    /// Issue #223: re-read the file the current snapshot was loaded
    /// from and re-run `compute_full` against the new contents.
    /// Emitted only when `state.snapshot.source_path` is `Some` — the
    /// renderer hides the button for in-memory snapshots.
    ReimportCurrent,
    /// Issue #223 follow-up: write the current snapshot's character
    /// back to disk as a `.mk2` so the user can keep a comparison
    /// build they constructed in-memory ("Snapshot current build")
    /// without retreating to the main Save flow. The host opens a
    /// file picker pre-populated with a sanitised label and writes
    /// `export_code(snapshot.character)`.
    SaveSnapshotToFile,
}

/// Issue #223 follow-up: reset the user's in-flight Compare-view state.
/// Clears the filter input and the sort selection. Preserves the
/// preferences the user opts into (`hide_zero_delta`, `export_format`),
/// the snapshot itself, and any in-flight `pending_relabel` so a
/// mid-rename isn't clobbered by the reset.
///
/// Mirrors the [`crate::calcs_tab::reset_calcs_view`] split between
/// "transient view state" (cleared) and "sticky preferences" (kept).
pub fn reset_compare_view(state: &mut CompareTabState) {
    state.filter.clear();
    state.sort_mode = CompareSortMode::default();
}

pub fn ui(
    ui: &mut egui::Ui,
    state: &mut CompareTabState,
    live_character: &Character,
    live_output: &Output,
    tree: &PassiveTree,
) -> Option<CompareAction> {
    let mut action: Option<CompareAction> = None;
    ui.horizontal(|ui| {
        ui.heading("Compare");
        ui.separator();
        if ui.button("Snapshot current build").clicked() {
            state.snapshot = Some(Snapshot {
                character: live_character.clone(),
                output: live_output.clone(),
                label: format_snapshot_label(live_character),
                source_path: None,
            });
            // The rename buffer (if any) was seeded from the previous
            // snapshot's label — drop it so the new snapshot doesn't
            // open the rename UI with stale text.
            state.pending_relabel = None;
        }
        if ui.button("Load comparison from file…").clicked() {
            action = Some(CompareAction::LoadFromFile);
        }
        // Issue #223: re-import button only renders when the current
        // snapshot was loaded from a file on disk (in-memory
        // "Snapshot current build" captures don't have a path to
        // re-read). Lets a user iterate against an external build
        // file and pull updates in without re-traversing the file
        // picker.
        if state
            .snapshot
            .as_ref()
            .and_then(|s| s.source_path.as_ref())
            .is_some()
            && ui
                .button("Re-import current")
                .on_hover_text(
                    "Re-read the snapshot's source file off disk and re-run \
                     compute_full against it. Useful when the saved build has \
                     changed elsewhere and you want to refresh the comparison.",
                )
                .clicked()
        {
            action = Some(CompareAction::ReimportCurrent);
        }
        // Issue #223 follow-up: persist the in-memory snapshot to a
        // `.mk2` file so a comparison build constructed via "Snapshot
        // current build" survives a restart without falling back to
        // the main Save flow.
        if state.snapshot.is_some()
            && ui
                .button("Save snapshot…")
                .on_hover_text(
                    "Write the snapshot's character to a `.mk2` file. The picker \
                     pre-populates with a sanitised version of the snapshot \
                     label.",
                )
                .clicked()
        {
            action = Some(CompareAction::SaveSnapshotToFile);
        }
        if state.snapshot.is_some() && ui.button("Clear snapshot").clicked() {
            state.snapshot = None;
            state.pending_relabel = None;
        }
    });

    let Some(snap) = state.snapshot.as_ref() else {
        ui.separator();
        ui.label(
            "Click \"Snapshot current build\" to capture the current stats. \
             After that, every change to the live build shows up here as a delta.\n\
             \n\
             Or click \"Load comparison from file…\" to compare against a saved \
             build (.mk2 / .xml / PoB share-code).",
        );
        return action;
    };

    ui.separator();
    // Issue #223 follow-up: inline rename for the snapshot label.
    // `pending_relabel` carries the in-flight buffer; Enter commits
    // via `sanitise_snapshot_label`, Escape clears without applying.
    let mut commit_label: Option<String> = None;
    ui.horizontal(|ui| {
        ui.label("Snapshot:");
        if let Some(buf) = state.pending_relabel.as_mut() {
            let resp = ui.add(
                egui::TextEdit::singleline(buf)
                    .desired_width(220.0)
                    .hint_text("Snapshot label"),
            );
            let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
            if enter {
                commit_label = Some(sanitise_snapshot_label(buf));
            }
            if enter || esc || resp.lost_focus() {
                state.pending_relabel = None;
            }
        } else {
            ui.weak(&snap.label);
            if ui
                .small_button("✎")
                .on_hover_text("Rename this snapshot")
                .clicked()
            {
                state.pending_relabel = Some(snap.label.clone());
            }
        }
    });
    if let (Some(new_label), Some(snap)) = (commit_label, state.snapshot.as_mut()) {
        snap.label = new_label;
    }
    let Some(snap) = state.snapshot.as_ref() else {
        return action;
    };
    // Issue #220 slice 1: surface the passive-tree diff between
    // snapshot and live. Pure data layer — the full in-tree overlay
    // (PoB's `TreeTab.lua:106-125` "compare to spec") is a follow-up
    // slice that needs a multi-spec model.
    let diff = tree_diff(&snap.character.allocated, &live_character.allocated);
    let added_color = egui::Color32::from_rgb(0x33, 0xFF, 0x77);
    let removed_color = egui::Color32::from_rgb(0xDD, 0x00, 0x22);
    ui.horizontal(|ui| {
        ui.label("Tree diff:");
        ui.colored_label(added_color, format!("+{} nodes", diff.added.len()));
        ui.label("/");
        ui.colored_label(removed_color, format!("-{} nodes", diff.removed.len()));
        ui.weak(format!("({} shared) vs snapshot", diff.common.len()));
    });
    // Issue #220 follow-up: expand the +N / -N counts into a
    // collapsing list of named nodes so the user can see which
    // notables / keystones actually changed. Closed by default since
    // a big spec swap can list 50+ entries.
    if !diff.added.is_empty() || !diff.removed.is_empty() {
        egui::CollapsingHeader::new("Show changed nodes")
            .id_salt("compare_tree_diff_details")
            .default_open(false)
            .show(ui, |ui| {
                if !diff.added.is_empty() {
                    ui.colored_label(added_color, format!("Added ({}):", diff.added.len()));
                    for label in tree_diff_node_labels(&diff.added, tree) {
                        ui.label(format!("  + {label}"));
                    }
                }
                if !diff.removed.is_empty() {
                    if !diff.added.is_empty() {
                        ui.add_space(4.0);
                    }
                    ui.colored_label(removed_color, format!("Removed ({}):", diff.removed.len()));
                    for label in tree_diff_node_labels(&diff.removed, tree) {
                        ui.label(format!("  − {label}"));
                    }
                }
            });
    }
    // Issue #223 follow-up: same idea, applied to the equipped item
    // set. Lists slot-by-slot which item changed since the snapshot.
    // Independent collapsing header so a build with both passives and
    // gear differences shows both panels separately.
    let item_changes = diff_item_sets(&snap.character.items, &live_character.items);
    ui.horizontal(|ui| {
        ui.label("Item diff:");
        ui.weak(format!(
            "{} slot(s) changed vs snapshot",
            item_changes.len()
        ));
    });
    if !item_changes.is_empty() {
        egui::CollapsingHeader::new("Show changed items")
            .id_salt("compare_item_diff_details")
            .default_open(false)
            .show(ui, |ui| {
                for change in &item_changes {
                    ui.horizontal(|ui| {
                        ui.weak(format!("{:>14}:", change.slot.label()));
                        ui.colored_label(removed_color, &change.from);
                        ui.label("→");
                        ui.colored_label(added_color, &change.to);
                    });
                }
            });
    }
    // Issue #223 follow-up: deferred-reset flag so the "Reset view"
    // button can sit inside the filter/sort row without contending with
    // the immutable `snap` borrow that the diff table still needs.
    let mut reset_view_after = false;
    ui.horizontal(|ui| {
        ui.label("Filter:");
        ui.add(
            egui::TextEdit::singleline(&mut state.filter)
                .desired_width(220.0)
                .hint_text("Life, FireResist, MainSkill, …"),
        );
        ui.checkbox(&mut state.hide_zero_delta, "Hide zero deltas");
        ui.separator();
        // Issue #223: sort selector. Three modes mirror PoB —
        // alphabetical (stable, easy to find a stat), absolute delta
        // (biggest swings first), percent delta (biggest relative
        // changes first).
        ui.label("Sort:");
        ui.selectable_value(&mut state.sort_mode, CompareSortMode::Key, "Stat")
            .on_hover_text("Alphabetical by stat name.");
        ui.selectable_value(&mut state.sort_mode, CompareSortMode::AbsDelta, "|Δ|")
            .on_hover_text("Descending by absolute delta — biggest swings first.");
        ui.selectable_value(&mut state.sort_mode, CompareSortMode::PercentDelta, "%Δ")
            .on_hover_text(
                "Descending by percent delta — biggest relative changes first. \
                 Rows where the snapshot was zero have no percent and sort \
                 to the bottom.",
            );
        ui.separator();
        // One-click reset for the in-flight view state (filter input +
        // sort mode). Sticky preferences like "Hide zero deltas" and
        // the export format stay, mirroring the Calcs-tab reset rule.
        let has_view_state =
            !state.filter.trim().is_empty() || state.sort_mode != CompareSortMode::default();
        if ui
            .add_enabled(has_view_state, egui::Button::new("Reset view"))
            .on_hover_text(
                "Clear the filter and reset the sort selector back to alphabetical. \
                 Preserves the snapshot, the \"Hide zero deltas\" toggle, and the \
                 export format.",
            )
            .clicked()
        {
            reset_view_after = true;
        }
    });
    ui.separator();

    let rows = ordered_diff_rows(
        &snap.output,
        live_output,
        &state.filter,
        state.hide_zero_delta,
        state.sort_mode,
    );

    // Issue #223 follow-up: paste-friendly export. Format combo +
    // Copy button. Markdown for write-ups / Discord; CSV for
    // spreadsheets; JSON for scripts diffing builds programmatically.
    ui.horizontal(|ui| {
        ui.label("Copy as:");
        egui::ComboBox::from_id_salt("compare_export_format")
            .selected_text(state.export_format.label())
            .show_ui(ui, |ui| {
                for fmt in [
                    CompareExportFormat::Markdown,
                    CompareExportFormat::Csv,
                    CompareExportFormat::Json,
                ] {
                    ui.selectable_value(&mut state.export_format, fmt, fmt.label());
                }
            });
        if ui
            .button("Copy")
            .on_hover_text(
                "Copy the filtered + sorted diff rows to the clipboard in \
                 the chosen format.",
            )
            .clicked()
        {
            let text = format_compare_export(&rows, state.export_format);
            ui.ctx().copy_text(text);
        }
        ui.weak(format!("{} rows", rows.len()));
    });

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Grid::new("compare_grid")
                .num_columns(5)
                .striped(true)
                .show(ui, |ui| {
                    ui.strong("Stat");
                    ui.strong("Snapshot");
                    ui.strong("Live");
                    ui.strong("Delta");
                    ui.strong("%Δ");
                    ui.end_row();
                    for (key, snap_v, live) in &rows {
                        let delta = live - snap_v;
                        ui.label(key);
                        ui.label(format_value(*snap_v));
                        ui.label(format_value(*live));
                        let color = if delta > 1e-6 {
                            egui::Color32::from_rgb(0x33, 0xFF, 0x77)
                        } else if delta < -1e-6 {
                            egui::Color32::from_rgb(0xDD, 0x00, 0x22)
                        } else {
                            ui.style().visuals.text_color()
                        };
                        ui.colored_label(color, format_delta(delta));
                        match percent_delta(*snap_v, *live) {
                            Some(p) => ui.colored_label(color, format_percent(p)),
                            None => ui.weak("—"),
                        };
                        ui.end_row();
                    }
                });
        });
    // Deferred view-reset: the diff table's `snap` borrow lifetime ends
    // here, so it's safe to take the mutable state path now.
    if reset_view_after {
        reset_compare_view(state);
    }
    action
}

/// Build a snapshot label for a character imported from disk. Public
/// so the host can use it after running the file load + compute.
#[must_use]
pub fn label_for(c: &Character) -> String {
    format_snapshot_label(c)
}

/// Issue #223: decide which import path to use for a build-file
/// payload. The contract mirrors the existing inline branch in the
/// desktop `LoadFromFile` handler — `MK2|...` is a JSON share code,
/// `<...>` is a PoB-XML document, anything else is treated as a raw
/// PoB share code (base64-of-zlib-of-XML). Pure / no I/O: takes the
/// already-read text and dispatches to the right importer.
///
/// The returned `Result` carries a plain `String` error so the
/// "re-import" handler can surface it through the existing status
/// bar without translating per-import-kind error types.
pub fn import_build_text(text: &str) -> Result<Character, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err("Nothing to import — paste an MK2 / PoB XML / PoB share code.".to_owned());
    }
    if trimmed.starts_with("MK2|") {
        pob_engine::import_code(trimmed).map_err(|e| e.to_string())
    } else if trimmed.starts_with('<') {
        pob_engine::import_pob_xml(trimmed).map_err(|e| e.to_string())
    } else {
        pob_engine::import_pob_code(trimmed).map_err(|e| e.to_string())
    }
}

fn format_snapshot_label(c: &Character) -> String {
    let asc = c.ascendancy.as_deref().unwrap_or("");
    let skill = c
        .main_skill
        .as_ref()
        .map(|m| m.skill_id.as_str())
        .unwrap_or("(no main skill)");
    if asc.is_empty() {
        format!("{} L{} — {}", c.class.0, c.level, skill)
    } else {
        format!("{} ({}) L{} — {}", c.class.0, asc, c.level, skill)
    }
}

fn format_value(v: f64) -> String {
    if v.abs() >= 1000.0 || v.fract() == 0.0 {
        format!("{:.0}", v)
    } else {
        format!("{:.2}", v)
    }
}

/// Issue #223: percent change from `snap` to `live`. `None` when
/// `snap == 0` (percent is undefined — the row is still meaningful but
/// can't carry a percent value, and division would otherwise produce
/// `inf`). Pulled out so the percent column and the percent-sort key
/// share one rule. `live - snap` is the underlying delta in absolute
/// units; the percent is `delta / snap * 100`.
#[must_use]
pub fn percent_delta(snap: f64, live: f64) -> Option<f64> {
    if snap.abs() < 1e-12 {
        return None;
    }
    Some((live - snap) / snap * 100.0)
}

/// Issue #223: ordered list of `(key, snap, live)` triples for the
/// compare table, with the user's sort mode applied. Pulled out of
/// the egui loop so the ordering rule is unit-testable in isolation
/// (the egui side just walks the returned vec).
///
/// `filter` is matched case-insensitively against each key; an empty
/// filter accepts every key. `hide_zero` drops rows whose absolute
/// delta is zero. Stable sub-order is alphabetical by key so two rows
/// with identical sort keys land in a deterministic position.
#[must_use]
pub fn ordered_diff_rows(
    snap: &Output,
    live: &Output,
    filter: &str,
    hide_zero: bool,
    sort_mode: CompareSortMode,
) -> Vec<(String, f64, f64)> {
    let filter_lc = filter.to_ascii_lowercase();
    let key_set: std::collections::BTreeSet<String> = live
        .iter()
        .map(|(k, _)| k.to_owned())
        .chain(snap.iter().map(|(k, _)| k.to_owned()))
        .filter(|k| filter_lc.is_empty() || k.to_ascii_lowercase().contains(&filter_lc))
        .collect();

    let mut rows: Vec<(String, f64, f64)> = key_set
        .into_iter()
        .filter_map(|k| {
            let live_v = live.try_get(&k).unwrap_or(0.0);
            let snap_v = snap.try_get(&k).unwrap_or(0.0);
            let delta = live_v - snap_v;
            if hide_zero && delta.abs() < 1e-6 {
                return None;
            }
            Some((k, snap_v, live_v))
        })
        .collect();

    match sort_mode {
        CompareSortMode::Key => {
            // BTreeSet already provided alphabetical order.
        }
        CompareSortMode::AbsDelta => {
            rows.sort_by(|a, b| {
                let da = (a.2 - a.1).abs();
                let db = (b.2 - b.1).abs();
                // Descending; ties broken alphabetically.
                db.partial_cmp(&da)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| a.0.cmp(&b.0))
            });
        }
        CompareSortMode::PercentDelta => {
            rows.sort_by(|a, b| {
                let pa = percent_delta(a.1, a.2).map(f64::abs);
                let pb = percent_delta(b.1, b.2).map(f64::abs);
                // Rows with no percent (snap == 0) sort below rows
                // that have one — they're inherently un-orderable
                // by percent, so push them to the bottom rather
                // than tying with literal zero.
                match (pa, pb) {
                    (Some(a), Some(b)) => b.partial_cmp(&a).unwrap_or(std::cmp::Ordering::Equal),
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    (None, None) => std::cmp::Ordering::Equal,
                }
                .then_with(|| a.0.cmp(&b.0))
            });
        }
    }
    rows
}

fn format_delta(v: f64) -> String {
    if v.abs() < 1e-6 {
        "0".to_owned()
    } else if v > 0.0 {
        format!("+{}", format_value(v))
    } else {
        format_value(v)
    }
}

/// Issue #223: render a percent delta. Large magnitudes round to an
/// integer (a `+412.5%` row reads as `+413%`); small magnitudes keep
/// one decimal so a 0.4% change isn't squashed to 0%.
fn format_percent(p: f64) -> String {
    if p.abs() < 0.05 {
        "0%".to_owned()
    } else if p.abs() >= 100.0 {
        if p > 0.0 {
            format!("+{p:.0}%")
        } else {
            format!("{p:.0}%")
        }
    } else if p > 0.0 {
        format!("+{p:.1}%")
    } else {
        format!("{p:.1}%")
    }
}

/// Issue #223 follow-up: turn a snapshot's display label into a safe
/// default filename. Strips characters that would trip the platform
/// file picker (`/`, `\`, `:`, `*`, `?`, `"`, `<`, `>`, `|`, control
/// chars) and collapses internal whitespace to single spaces so a
/// label like "Witch Arc (boss / mapping)" becomes
/// `Witch Arc (boss mapping).mk2`.
///
/// Empty input falls back to `snapshot.mk2`. Capped at 64 chars
/// before the extension so an absurdly long label doesn't blow past
/// per-platform filename limits (Windows is 255 chars; we leave room
/// for the directory and a leading marker).
#[must_use]
pub fn default_snapshot_filename(label: &str) -> String {
    const FORBIDDEN: &[char] = &['/', '\\', ':', '*', '?', '"', '<', '>', '|'];
    let cleaned: String = label
        .chars()
        .filter(|c| !c.is_control() && !FORBIDDEN.contains(c))
        .collect();
    let collapsed: String = cleaned.split_whitespace().collect::<Vec<&str>>().join(" ");
    let trimmed = collapsed.trim();
    let stem = if trimmed.is_empty() {
        "snapshot"
    } else if trimmed.chars().count() > 64 {
        // Truncate to char-boundary at 64 graphemes (approximate via
        // chars — labels are ASCII-dominant in practice).
        let head: String = trimmed.chars().take(64).collect();
        return format!("{}.mk2", head.trim_end());
    } else {
        trimmed
    };
    format!("{stem}.mk2")
}

/// Issue #223 follow-up: trim + sanitise a user-typed snapshot label.
/// Empty / whitespace-only input falls back to a placeholder so the
/// compare panel always has *something* to render. Pure / testable so
/// the rename inline-edit can apply the same rule on commit.
#[must_use]
pub fn sanitise_snapshot_label(input: &str) -> String {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        "(unnamed snapshot)".to_owned()
    } else {
        trimmed.to_owned()
    }
}

/// Issue #223 follow-up: one slot that changed between snapshot and
/// live build. `from` / `to` carry user-facing labels (item name +
/// base, or `(empty)` when nothing is equipped). Generated by
/// [`diff_item_sets`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemDiffEntry {
    pub slot: Slot,
    pub from: String,
    pub to: String,
}

/// Issue #223 follow-up: per-slot label for an equipped item — `Name
/// (Base)` for unique-/rare-style "the name and the base differ", else
/// just the base name. Pulled out so the Compare-tab diff and the
/// future item-set switcher tooltip can share one format.
#[must_use]
pub fn format_item_label(item: &Item) -> String {
    let trimmed_name = item.name.trim();
    if !trimmed_name.is_empty() && trimmed_name != item.base_name.trim() {
        format!("{} ({})", trimmed_name, item.base_name)
    } else {
        item.base_name.clone()
    }
}

/// Issue #223 follow-up: list the slots that differ between two
/// [`ItemSet`]s. Used by the Compare tab to surface "swapped Helmet
/// from X to Y" alongside the existing tree-diff. Pure / no egui;
/// callers render the returned vec.
///
/// Identity is checked by label equality — same as what the UI shows —
/// so two rare items with the same generated name + base read as
/// "unchanged" here. A future slice could deepen the comparison to
/// the full mod-line set; today's heuristic is correct for the
/// "swapped one slot" case the user cares about.
#[must_use]
pub fn diff_item_sets(a: &ItemSet, b: &ItemSet) -> Vec<ItemDiffEntry> {
    let mut out = Vec::new();
    for slot in Slot::all() {
        let from_label = a.get(*slot).map(format_item_label);
        let to_label = b.get(*slot).map(format_item_label);
        if from_label != to_label {
            out.push(ItemDiffEntry {
                slot: *slot,
                from: from_label.unwrap_or_else(|| "(empty)".to_owned()),
                to: to_label.unwrap_or_else(|| "(empty)".to_owned()),
            });
        }
    }
    out
}

/// Issue #220 follow-up: look up display labels for a list of node ids
/// against `tree.nodes`. Nodes with a `name` use it directly; unnamed
/// or unknown ids fall back to `#<id>` so the list still gives the
/// user something to click into. Pure / no egui — the call site
/// renders the returned labels in a `CollapsingHeader`.
#[must_use]
pub fn tree_diff_node_labels(ids: &[NodeId], tree: &PassiveTree) -> Vec<String> {
    ids.iter().map(|id| node_label(*id, tree)).collect()
}

fn node_label(id: NodeId, tree: &PassiveTree) -> String {
    tree.nodes
        .get(&id)
        .and_then(|n| n.name.clone())
        .unwrap_or_else(|| format!("#{id}"))
}

/// Issue #223 follow-up: serialisation choice for the Compare-tab
/// "Copy as..." button. Markdown is the original (and best for
/// Discord / GitHub pastes); CSV suits spreadsheet imports; JSON is
/// machine-friendly for scripts diffing builds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CompareExportFormat {
    /// Pipe-delimited Markdown table.
    #[default]
    Markdown,
    /// Comma-separated values with a header row. Numbers are emitted
    /// in `format_value`'s rounded form so the export matches what
    /// the user sees on screen.
    Csv,
    /// JSON array of `{ "stat", "snap", "live", "delta", "percent" }`
    /// objects. `percent` is `null` for rows with a zero snapshot
    /// value (matches the on-screen `—` fallback).
    Json,
}

impl CompareExportFormat {
    /// Human-readable label for the format combo.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Markdown => "Markdown",
            Self::Csv => "CSV",
            Self::Json => "JSON",
        }
    }
}

/// Issue #223 follow-up: render `rows` in the requested format. Pure
/// dispatch over [`format_compare_markdown`] /
/// [`format_compare_csv`] / [`format_compare_json`] so the UI side
/// stays a thin combo + button.
#[must_use]
pub fn format_compare_export(rows: &[(String, f64, f64)], format: CompareExportFormat) -> String {
    match format {
        CompareExportFormat::Markdown => format_compare_markdown(rows),
        CompareExportFormat::Csv => format_compare_csv(rows),
        CompareExportFormat::Json => format_compare_json(rows),
    }
}

/// Issue #223 follow-up: comma-separated values with a header row.
/// Stat labels are quoted (defensive against names that contain a
/// comma); numeric columns are unquoted. Percent column uses the
/// `format_percent` rendering, falling back to an empty string when
/// the snapshot value is zero (CSV doesn't have a "null" literal).
#[must_use]
pub fn format_compare_csv(rows: &[(String, f64, f64)]) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    out.push_str("Stat,Snapshot,Live,Delta,PercentDelta\n");
    for (key, snap, live) in rows {
        let delta = live - snap;
        let pct = match percent_delta(*snap, *live) {
            Some(p) => format_percent(p),
            None => String::new(),
        };
        let _ = writeln!(
            out,
            // Escape embedded `"` per RFC 4180 doubling rule.
            "\"{}\",{},{},{},{}",
            key.replace('"', "\"\""),
            format_value(*snap).trim(),
            format_value(*live).trim(),
            format_delta(delta).trim(),
            pct,
        );
    }
    out
}

/// Issue #223 follow-up: JSON array of compact records. Numeric
/// columns are emitted as numbers (not strings) so a downstream
/// `jq` / `python -m json.tool` pipe can pivot or sort directly.
/// `percent` is `null` for zero-snapshot rows.
#[must_use]
pub fn format_compare_json(rows: &[(String, f64, f64)]) -> String {
    let mut out = String::from("[\n");
    for (i, (key, snap, live)) in rows.iter().enumerate() {
        let delta = live - snap;
        let pct = percent_delta(*snap, *live);
        out.push_str("  {\"stat\":");
        out.push('"');
        // JSON string escaping: `\` and `"` get backslash escapes;
        // other control chars are pass-through (stat keys are
        // engine-generated ASCII identifiers in practice).
        out.push_str(&key.replace('\\', "\\\\").replace('"', "\\\""));
        out.push_str("\",\"snap\":");
        out.push_str(&json_num(*snap));
        out.push_str(",\"live\":");
        out.push_str(&json_num(*live));
        out.push_str(",\"delta\":");
        out.push_str(&json_num(delta));
        out.push_str(",\"percent\":");
        match pct {
            Some(p) => out.push_str(&json_num(p)),
            None => out.push_str("null"),
        }
        out.push('}');
        if i + 1 < rows.len() {
            out.push(',');
        }
        out.push('\n');
    }
    out.push(']');
    out
}

/// Internal helper: format an `f64` for JSON, preferring an integer
/// form when the value rounds cleanly and falling back to 4 decimal
/// places otherwise. Avoids `serde_json::to_string`'s trailing `.0`
/// on small integers and keeps the output stable across platforms.
fn json_num(v: f64) -> String {
    if v.is_nan() || v.is_infinite() {
        // Defensive — JSON has no NaN/Infinity literals; emit `null`.
        return "null".to_owned();
    }
    if v.fract().abs() < 1e-9 {
        format!("{}", v.round() as i64)
    } else {
        let s = format!("{v:.4}");
        // Strip trailing zeroes after the decimal point so `0.5000`
        // reads as `0.5` — but keep at least one digit after the dot.
        let trimmed = s.trim_end_matches('0').trim_end_matches('.');
        if trimmed == s.trim() {
            s
        } else {
            trimmed.to_owned()
        }
    }
}

/// Issue #223 follow-up: render the compare-table rows as a Markdown
/// table the user can paste into a build write-up / Discord / GitHub
/// issue. Uses the same `format_value` / `format_delta` /
/// `format_percent` helpers as the on-screen table so the exported
/// numbers match what the user sees.
///
/// Pure / no egui — the call site copies the returned string into the
/// clipboard.
#[must_use]
pub fn format_compare_markdown(rows: &[(String, f64, f64)]) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    out.push_str("| Stat | Snapshot | Live | Δ | %Δ |\n");
    out.push_str("|---|---:|---:|---:|---:|\n");
    for (key, snap, live) in rows {
        let delta = live - snap;
        let pct = match percent_delta(*snap, *live) {
            Some(p) => format_percent(p),
            None => "—".to_owned(),
        };
        let _ = writeln!(
            out,
            "| {} | {} | {} | {} | {} |",
            key,
            format_value(*snap),
            format_value(*live),
            format_delta(delta),
            pct,
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delta_formatting_signs_and_zero() {
        // Positive deltas get a leading "+", negatives keep the minus
        // sign from format_value, near-zero collapses to "0".
        assert_eq!(format_delta(0.0), "0");
        assert_eq!(format_delta(1e-9), "0");
        assert_eq!(format_delta(-1e-9), "0");
        assert_eq!(format_delta(50.0), "+50");
        assert_eq!(format_delta(-50.0), "-50");
        assert_eq!(format_delta(0.25), "+0.25");
        assert_eq!(format_delta(-0.25), "-0.25");
        assert_eq!(format_delta(1500.5), "+1500");
    }

    #[test]
    fn value_formatting_picks_integer_or_two_decimals() {
        assert_eq!(format_value(0.0), "0");
        assert_eq!(format_value(50.0), "50");
        assert_eq!(format_value(50.25), "50.25");
        // Big magnitude → integer form regardless of fraction.
        assert_eq!(format_value(1234.56), "1235");
        assert_eq!(format_value(-1234.56), "-1235");
    }

    #[test]
    fn percent_delta_returns_none_for_zero_snapshot() {
        // Division-by-zero guard — percent isn't meaningful when the
        // snapshot value is zero. Callers render "—" for the percent
        // column in that case.
        assert_eq!(percent_delta(0.0, 10.0), None);
        assert_eq!(percent_delta(-1e-15, 10.0), None);
    }

    #[test]
    fn percent_delta_positive_and_negative_cases() {
        // 50 → 100 is a +100% change; 100 → 50 is -50%.
        assert!((percent_delta(50.0, 100.0).unwrap() - 100.0).abs() < 1e-9);
        assert!((percent_delta(100.0, 50.0).unwrap() - -50.0).abs() < 1e-9);
        // Identity: same value → 0%.
        assert_eq!(percent_delta(42.0, 42.0), Some(0.0));
    }

    #[test]
    fn format_percent_collapses_near_zero_and_signs_otherwise() {
        assert_eq!(format_percent(0.0), "0%");
        assert_eq!(format_percent(0.04), "0%");
        assert_eq!(format_percent(-0.04), "0%");
        assert_eq!(format_percent(5.5), "+5.5%");
        assert_eq!(format_percent(-5.5), "-5.5%");
        // Large magnitude → integer form so the column doesn't blow
        // up on a "snapshot was 1, live is 50" row.
        assert_eq!(format_percent(4900.0), "+4900%");
        assert_eq!(format_percent(-200.0), "-200%");
    }

    fn out(pairs: &[(&str, f64)]) -> Output {
        let mut o = Output::default();
        for (k, v) in pairs {
            o.set(*k, *v);
        }
        o
    }

    #[test]
    fn ordered_diff_rows_alphabetical_by_key_under_key_mode() {
        let snap = out(&[("Life", 100.0), ("FireResist", 30.0)]);
        let live = out(&[("Life", 150.0), ("FireResist", 75.0)]);
        let rows = ordered_diff_rows(&snap, &live, "", false, CompareSortMode::Key);
        let order: Vec<&str> = rows.iter().map(|(k, _, _)| k.as_str()).collect();
        // BTreeSet ordering → alphabetical.
        assert_eq!(order, vec!["FireResist", "Life"]);
    }

    #[test]
    fn ordered_diff_rows_absdelta_orders_biggest_swing_first() {
        let snap = out(&[("Life", 100.0), ("FireResist", 30.0), ("Mana", 50.0)]);
        let live = out(&[("Life", 1000.0), ("FireResist", 75.0), ("Mana", 49.0)]);
        let rows = ordered_diff_rows(&snap, &live, "", false, CompareSortMode::AbsDelta);
        let order: Vec<&str> = rows.iter().map(|(k, _, _)| k.as_str()).collect();
        // |Life Δ| = 900, |FireResist Δ| = 45, |Mana Δ| = 1.
        assert_eq!(order, vec!["Life", "FireResist", "Mana"]);
    }

    #[test]
    fn ordered_diff_rows_percentdelta_pushes_zero_snapshot_rows_to_bottom() {
        // Mana goes from 0 → 50 (percent undefined) — should sort
        // *after* every row with a defined percent, even when the
        // absolute swing is large.
        let snap = out(&[("Life", 100.0), ("FireResist", 30.0), ("Mana", 0.0)]);
        let live = out(&[("Life", 150.0), ("FireResist", 60.0), ("Mana", 50.0)]);
        let rows = ordered_diff_rows(&snap, &live, "", false, CompareSortMode::PercentDelta);
        let order: Vec<&str> = rows.iter().map(|(k, _, _)| k.as_str()).collect();
        // FireResist: +100%. Life: +50%. Mana: undefined → bottom.
        assert_eq!(order, vec!["FireResist", "Life", "Mana"]);
    }

    fn make_tree(named: &[(NodeId, &str)]) -> PassiveTree {
        use ahash::HashMap;
        use pob_data::{Node, NodeKind, TreeConstants, TreePoints};
        let mut nodes: HashMap<NodeId, Node> = HashMap::default();
        for (id, name) in named {
            nodes.insert(
                *id,
                Node {
                    id: *id,
                    name: Some((*name).to_owned()),
                    icon: None,
                    ascendancy_name: None,
                    stats: vec![],
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
        PassiveTree {
            version: "test".into(),
            tree: "test".into(),
            classes: vec![],
            groups: HashMap::default(),
            nodes,
            jewel_slots: vec![],
            min_x: 0,
            min_y: 0,
            max_x: 0,
            max_y: 0,
            constants: TreeConstants {
                skills_per_orbit: vec![],
                orbit_radii: vec![],
                classes: HashMap::default(),
                character_attributes: HashMap::default(),
                pss_centre_inner_radius: None,
            },
            points: TreePoints::default(),
        }
    }

    #[test]
    fn default_snapshot_filename_falls_back_for_empty_input() {
        // Empty / whitespace-only label maps to a stable default so
        // the file picker isn't pre-populated with junk.
        assert_eq!(default_snapshot_filename(""), "snapshot.mk2");
        assert_eq!(default_snapshot_filename("   "), "snapshot.mk2");
    }

    #[test]
    fn default_snapshot_filename_strips_filesystem_forbidden_chars() {
        // Windows/macOS/Linux all forbid some of `/ \ : * ? " < > |`
        // in filenames. Strip every one of them and collapse the
        // resulting double-space.
        let out = default_snapshot_filename(r#"My/build:test*?"<>|"#);
        assert_eq!(out, "Mybuildtest.mk2");
    }

    #[test]
    fn default_snapshot_filename_collapses_interior_whitespace() {
        // Tabs and runs of spaces collapse to single spaces so the
        // resulting filename reads cleanly.
        let out = default_snapshot_filename("Witch  Arc\t L92");
        assert_eq!(out, "Witch Arc L92.mk2");
    }

    #[test]
    fn default_snapshot_filename_truncates_long_labels() {
        // Defends against a 200-char rare-item-style label blowing
        // past per-platform filename caps (Windows: 255 chars).
        let huge = "a".repeat(200);
        let out = default_snapshot_filename(&huge);
        // 64 chars + ".mk2" suffix = 68 bytes. Producer always lowercases
        // the extension, so the strict ends_with is correct here.
        assert!(std::path::Path::new(&out)
            .extension()
            .is_some_and(|e| e == "mk2"));
        assert!(
            out.len() <= 68,
            "expected truncation to 64+suffix, got {} bytes",
            out.len()
        );
    }

    #[test]
    fn default_snapshot_filename_preserves_unicode_letters() {
        // Stripping should be precise: real text characters survive,
        // only the forbidden punctuation drops.
        let out = default_snapshot_filename("Boss — Ngamahu");
        assert_eq!(out, "Boss — Ngamahu.mk2");
    }

    #[test]
    fn sanitise_snapshot_label_passes_through_non_empty_trimmed() {
        // Standard case: a real label is preserved verbatim, modulo
        // leading / trailing whitespace.
        assert_eq!(sanitise_snapshot_label("Witch Arc L92"), "Witch Arc L92");
        assert_eq!(sanitise_snapshot_label("   Boss Setup   "), "Boss Setup");
    }

    #[test]
    fn sanitise_snapshot_label_falls_back_for_empty_or_blank() {
        // Empty / whitespace-only would render as a void in the
        // Compare panel — substitute the documented placeholder.
        assert_eq!(sanitise_snapshot_label(""), "(unnamed snapshot)");
        assert_eq!(sanitise_snapshot_label("   "), "(unnamed snapshot)");
        assert_eq!(sanitise_snapshot_label("\t\n"), "(unnamed snapshot)");
    }

    #[test]
    fn sanitise_snapshot_label_preserves_interior_whitespace() {
        // Only outer whitespace is trimmed — the user's spacing
        // between words is left alone.
        assert_eq!(
            sanitise_snapshot_label("Arc  with  Hierophant"),
            "Arc  with  Hierophant"
        );
    }

    fn item_with(name: &str, base: &str) -> Item {
        Item {
            name: name.to_owned(),
            base_name: base.to_owned(),
            ..Default::default()
        }
    }

    #[test]
    fn format_item_label_combines_name_and_base_when_distinct() {
        let unique = item_with("Headhunter", "Leather Belt");
        assert_eq!(format_item_label(&unique), "Headhunter (Leather Belt)");
    }

    #[test]
    fn format_item_label_drops_redundant_name_matching_base() {
        // A bare base (white item) carries name == base; the label
        // collapses to just the base so the UI doesn't repeat the word.
        let plain = item_with("Leather Belt", "Leather Belt");
        assert_eq!(format_item_label(&plain), "Leather Belt");
        // Whitespace-only name is also "redundant".
        let blank = item_with("   ", "Leather Belt");
        assert_eq!(format_item_label(&blank), "Leather Belt");
    }

    #[test]
    fn diff_item_sets_reports_swapped_slot() {
        // Same slot, different item → one entry with from/to.
        let mut a = ItemSet::new();
        a.equip(Slot::Helmet, item_with("Goldrim", "Leather Cap"));
        let mut b = ItemSet::new();
        b.equip(
            Slot::Helmet,
            item_with("Devoto's Devotion", "Nightmare Bascinet"),
        );
        let diff = diff_item_sets(&a, &b);
        assert_eq!(diff.len(), 1);
        assert_eq!(diff[0].slot, Slot::Helmet);
        assert!(diff[0].from.contains("Goldrim"));
        assert!(diff[0].to.contains("Devoto"));
    }

    #[test]
    fn diff_item_sets_reports_equip_and_unequip() {
        // A slot empty on one side and full on the other still counts
        // as a change, with `(empty)` filling in the missing label.
        let a = ItemSet::new();
        let mut b = ItemSet::new();
        b.equip(Slot::Amulet, item_with("Stranglegasp", "Onyx Amulet"));
        let added = diff_item_sets(&a, &b);
        assert_eq!(added.len(), 1);
        assert_eq!(added[0].from, "(empty)");
        assert!(added[0].to.contains("Stranglegasp"));
        // Mirror — unequip case.
        let removed = diff_item_sets(&b, &a);
        assert_eq!(removed.len(), 1);
        assert!(removed[0].from.contains("Stranglegasp"));
        assert_eq!(removed[0].to, "(empty)");
    }

    #[test]
    fn diff_item_sets_returns_empty_when_sets_match() {
        let mut a = ItemSet::new();
        a.equip(Slot::Helmet, item_with("Goldrim", "Leather Cap"));
        a.equip(Slot::Belt, item_with("Headhunter", "Leather Belt"));
        let b = a.clone();
        assert!(diff_item_sets(&a, &b).is_empty());
    }

    #[test]
    fn diff_item_sets_walks_every_slot_in_canonical_order() {
        // Multiple changes across distinct slots — the result should
        // preserve `Slot::all()`'s left-to-right order so the UI
        // renders them consistently across frames.
        let mut a = ItemSet::new();
        a.equip(Slot::Helmet, item_with("Old Helm", "Leather Cap"));
        a.equip(Slot::Belt, item_with("Old Belt", "Leather Belt"));
        let mut b = ItemSet::new();
        b.equip(Slot::Helmet, item_with("New Helm", "Nightmare Bascinet"));
        b.equip(Slot::Belt, item_with("New Belt", "Stygian Vise"));
        let diff = diff_item_sets(&a, &b);
        let slots: Vec<Slot> = diff.iter().map(|d| d.slot).collect();
        assert_eq!(slots, vec![Slot::Helmet, Slot::Belt]);
    }

    #[test]
    fn tree_diff_node_labels_uses_node_name_when_present() {
        // Looks up names for known node ids; preserves input order.
        let tree = make_tree(&[(1, "Pure Talent"), (2, "Resolute Technique")]);
        let labels = tree_diff_node_labels(&[1, 2], &tree);
        assert_eq!(labels, vec!["Pure Talent", "Resolute Technique"]);
    }

    #[test]
    fn tree_diff_node_labels_falls_back_to_hash_id_for_unknown() {
        // Stale snapshot vs current tree version can list a node id
        // that the tree no longer carries — fall back to `#<id>` so
        // the row still gives the user something to recognise.
        let tree = make_tree(&[(1, "Pure Talent")]);
        let labels = tree_diff_node_labels(&[1, 999], &tree);
        assert_eq!(labels, vec!["Pure Talent", "#999"]);
    }

    #[test]
    fn tree_diff_node_labels_empty_input_returns_empty_vec() {
        let tree = make_tree(&[]);
        let labels = tree_diff_node_labels(&[], &tree);
        assert!(labels.is_empty());
    }

    #[test]
    fn format_compare_csv_emits_header_and_quoted_keys() {
        // Stat labels are quoted; numeric columns aren't. Confirms the
        // header line is present even for empty input.
        let csv = format_compare_csv(&[]);
        assert!(csv.starts_with("Stat,Snapshot,Live,Delta,PercentDelta\n"));
        let rows = vec![("Life".to_owned(), 100.0, 150.0)];
        let csv = format_compare_csv(&rows);
        assert!(
            csv.contains("\"Life\",100,150,+50,+50.0%"),
            "expected quoted-key row, got:\n{csv}"
        );
    }

    #[test]
    fn format_compare_csv_doubles_embedded_quotes_per_rfc_4180() {
        // A stat key containing a literal `"` should produce `""` in
        // the CSV output so importers reading RFC 4180 don't choke.
        let rows = vec![("Weird\"Key".to_owned(), 1.0, 2.0)];
        let csv = format_compare_csv(&rows);
        assert!(
            csv.contains("\"Weird\"\"Key\""),
            "expected RFC 4180 quote doubling, got:\n{csv}"
        );
    }

    #[test]
    fn format_compare_csv_leaves_percent_empty_when_snap_is_zero() {
        // CSV has no "null" literal — the empty-string fallback keeps
        // the column count consistent across rows.
        let rows = vec![("Mana".to_owned(), 0.0, 50.0)];
        let csv = format_compare_csv(&rows);
        assert!(
            csv.contains("\"Mana\",0,50,+50,\n"),
            "expected empty percent cell, got:\n{csv}"
        );
    }

    #[test]
    fn format_compare_json_emits_well_formed_array() {
        // Empty input → empty array (well, `[\n]`). Confirms the
        // structural prelude / closer is always emitted.
        let json = format_compare_json(&[]);
        assert!(json.starts_with("[\n"), "got {json:?}");
        assert!(json.ends_with(']'), "got {json:?}");
    }

    #[test]
    fn format_compare_json_renders_numeric_columns_as_numbers() {
        let rows = vec![("Life".to_owned(), 100.0, 150.0)];
        let json = format_compare_json(&rows);
        assert!(json.contains("\"stat\":\"Life\""));
        assert!(json.contains("\"snap\":100"));
        assert!(json.contains("\"live\":150"));
        assert!(json.contains("\"delta\":50"));
        assert!(json.contains("\"percent\":50"));
        // Not a string-quoted number.
        assert!(!json.contains("\"100\""), "snap should be a number: {json}");
    }

    #[test]
    fn format_compare_json_emits_null_percent_for_zero_snap() {
        // JSON's `null` cleanly distinguishes "undefined percent" from
        // a real 0% change, matching the on-screen `—` fallback.
        let rows = vec![("Mana".to_owned(), 0.0, 50.0)];
        let json = format_compare_json(&rows);
        assert!(
            json.contains("\"percent\":null"),
            "zero-snap should emit null percent, got:\n{json}"
        );
    }

    #[test]
    fn format_compare_json_escapes_quotes_in_stat_keys() {
        // Engine-generated keys are ASCII identifiers, but defend
        // against a hand-edited one with embedded quotes anyway.
        let rows = vec![("Weird\"Key".to_owned(), 1.0, 2.0)];
        let json = format_compare_json(&rows);
        assert!(
            json.contains("\"stat\":\"Weird\\\"Key\""),
            "expected backslash-escaped quote, got:\n{json}"
        );
    }

    #[test]
    fn format_compare_export_dispatches_to_each_format() {
        // End-to-end: every format produces a non-empty result for the
        // same input. Pins that the dispatch wires the right helper.
        let rows = vec![("Life".to_owned(), 100.0, 150.0)];
        for format in [
            CompareExportFormat::Markdown,
            CompareExportFormat::Csv,
            CompareExportFormat::Json,
        ] {
            let out = format_compare_export(&rows, format);
            assert!(!out.is_empty(), "{format:?} produced empty output");
            assert!(out.contains("150"), "{format:?} missing live value: {out}");
        }
    }

    #[test]
    fn compare_export_format_default_is_markdown() {
        // Markdown is the original (and most-used) format — pre-this
        // slice it was the only option.
        assert_eq!(
            CompareExportFormat::default(),
            CompareExportFormat::Markdown
        );
    }

    #[test]
    fn format_compare_markdown_emits_header_and_separator() {
        // Empty input still produces the table preamble so a pasted
        // table never collapses to bare text.
        let md = format_compare_markdown(&[]);
        let lines: Vec<&str> = md.lines().collect();
        assert_eq!(
            lines.len(),
            2,
            "empty input should leave just header + separator: {md:?}"
        );
        assert!(lines[0].starts_with("| Stat |"));
        assert!(lines[1].contains("---"));
    }

    #[test]
    fn format_compare_markdown_row_shapes_values_signs_and_percent() {
        let rows = vec![
            ("Life".to_owned(), 100.0, 150.0),
            ("FireResist".to_owned(), 30.0, 30.0),
        ];
        let md = format_compare_markdown(&rows);
        // Life row carries the signed delta and a positive percent.
        assert!(
            md.contains("| Life | 100 | 150 | +50 | +50.0% |"),
            "Life row should carry +50 / +50%, got:\n{md}"
        );
        // Identical-value row reads as a zero delta and 0% percent.
        assert!(
            md.contains("| FireResist | 30 | 30 | 0 | 0% |"),
            "FireResist row should collapse to zeros, got:\n{md}"
        );
    }

    #[test]
    fn format_compare_markdown_uses_em_dash_when_snap_is_zero() {
        // Percent column has no meaningful value when snapshot is zero;
        // the formatter falls back to `—` so the column stays aligned
        // and doesn't render "+inf%" / "NaN".
        let rows = vec![("Mana".to_owned(), 0.0, 50.0)];
        let md = format_compare_markdown(&rows);
        assert!(
            md.contains("| Mana | 0 | 50 | +50 | — |"),
            "zero-snap percent should render as em-dash, got:\n{md}"
        );
    }

    #[test]
    fn format_compare_markdown_preserves_caller_order() {
        // The formatter doesn't re-sort — `ordered_diff_rows` already
        // applied the user's chosen sort, so the export should mirror
        // the on-screen ordering.
        let rows = vec![("B".to_owned(), 1.0, 2.0), ("A".to_owned(), 1.0, 2.0)];
        let md = format_compare_markdown(&rows);
        let body_lines: Vec<&str> = md.lines().skip(2).collect();
        assert!(body_lines[0].contains("| B |"));
        assert!(body_lines[1].contains("| A |"));
    }

    #[test]
    fn ordered_diff_rows_filter_is_case_insensitive() {
        let snap = out(&[("Life", 100.0), ("FireResist", 30.0)]);
        let live = out(&[("Life", 150.0), ("FireResist", 60.0)]);
        let rows = ordered_diff_rows(&snap, &live, "FIRE", false, CompareSortMode::Key);
        let order: Vec<&str> = rows.iter().map(|(k, _, _)| k.as_str()).collect();
        assert_eq!(order, vec!["FireResist"]);
    }

    #[test]
    fn import_build_text_dispatches_on_mk2_prefix() {
        // Issue #223: round-trip a Character through `MK2|...` and
        // confirm the helper picks the JSON share-code path.
        let mut c = pob_engine::Character::new(pob_engine::ClassRef::ranger(), 67);
        c.notes = "round trip via mk2".into();
        let code = pob_engine::export_code(&c).expect("export");
        let back = import_build_text(&code).expect("import");
        assert_eq!(back.level, 67);
        assert_eq!(back.notes, "round trip via mk2");
    }

    #[test]
    fn import_build_text_dispatches_on_xml_prefix() {
        // A document starting with `<` lands on the PoB-XML path;
        // confirm via the round-trip of `export_pob_xml`.
        let c = pob_engine::Character::new(pob_engine::ClassRef::ranger(), 92);
        let xml = pob_engine::export_pob_xml(&c);
        let back = import_build_text(&xml).expect("import xml");
        assert_eq!(back.class.0, "Ranger");
        assert_eq!(back.level, 92);
    }

    #[test]
    fn import_build_text_returns_string_error_on_garbage() {
        // The plain-text return type lets the caller surface the
        // error through `app.status_message` without a per-import
        // adapter; confirm garbage input produces an `Err` rather
        // than panicking.
        assert!(import_build_text("not a build").is_err());
    }

    #[test]
    fn import_build_text_empty_input_surfaces_friendly_message() {
        // The pre-dispatch empty-input guard turns "you pasted
        // nothing" from an opaque zlib/base64 decode error into a
        // sentence the user can act on. The exact wording is part of
        // the contract — the status banner copy-pastes it verbatim
        // (no per-call-site reformatting).
        let err = import_build_text("").expect_err("empty input is an error");
        assert!(
            err.contains("Nothing to import"),
            "friendly empty-input message expected, got {err:?}"
        );
        let err = import_build_text("   \n\t  ").expect_err("whitespace-only input is an error");
        assert!(
            err.contains("Nothing to import"),
            "trim semantics should send whitespace-only through the same path: {err:?}"
        );
    }

    #[test]
    fn import_build_text_trims_surrounding_whitespace() {
        // File reads through `std::fs::read_to_string` sometimes
        // carry a trailing newline. The helper trims before
        // dispatching so the prefix check works against the actual
        // first non-whitespace character.
        let c = pob_engine::Character::new(pob_engine::ClassRef::ranger(), 1);
        let code = pob_engine::export_code(&c).expect("export");
        let padded = format!("\n\n  {code}\n");
        assert!(import_build_text(&padded).is_ok());
    }

    #[test]
    fn ordered_diff_rows_hide_zero_drops_unchanged_keys() {
        let snap = out(&[("Life", 100.0), ("Mana", 50.0)]);
        let live = out(&[("Life", 150.0), ("Mana", 50.0)]);
        let rows = ordered_diff_rows(&snap, &live, "", true, CompareSortMode::Key);
        let order: Vec<&str> = rows.iter().map(|(k, _, _)| k.as_str()).collect();
        // Mana is unchanged; only Life survives.
        assert_eq!(order, vec!["Life"]);
    }

    // ─── reset_compare_view ──────────────────────────────────────────────

    #[test]
    fn reset_compare_view_clears_filter_and_sort() {
        // Transient view state — filter input + non-default sort — must
        // both drop back to the cold-open defaults so the button delivers
        // on its label.
        let mut state = CompareTabState {
            filter: "Life".to_owned(),
            sort_mode: CompareSortMode::AbsDelta,
            ..Default::default()
        };
        reset_compare_view(&mut state);
        assert!(state.filter.is_empty(), "filter should be cleared");
        assert_eq!(state.sort_mode, CompareSortMode::default());
    }

    #[test]
    fn reset_compare_view_preserves_sticky_preferences() {
        // `hide_zero_delta` and `export_format` are user preferences,
        // not transient view state — they survive the reset so a power
        // user who configured their preferred view doesn't have to
        // re-toggle them after clearing a filter.
        let mut state = CompareTabState {
            filter: "x".to_owned(),
            hide_zero_delta: true,
            export_format: CompareExportFormat::Csv,
            ..Default::default()
        };
        reset_compare_view(&mut state);
        assert!(
            state.hide_zero_delta,
            "hide_zero_delta is a sticky preference"
        );
        assert_eq!(state.export_format, CompareExportFormat::Csv);
    }

    #[test]
    fn reset_compare_view_preserves_pending_relabel() {
        // The in-flight rename buffer is held outside the view-state
        // bucket — a mid-rename should survive the reset so the user
        // doesn't lose typed text. The snapshot itself sits behind a
        // host-supplied path and isn't carried through this state
        // mutation either way.
        let mut state = CompareTabState {
            filter: "x".to_owned(),
            pending_relabel: Some("WIP".to_owned()),
            ..Default::default()
        };
        reset_compare_view(&mut state);
        assert_eq!(
            state.pending_relabel.as_deref(),
            Some("WIP"),
            "rename buffer must survive a view reset"
        );
    }
}
