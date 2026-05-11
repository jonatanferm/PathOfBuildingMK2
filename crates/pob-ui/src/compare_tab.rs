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
}

pub fn ui(
    ui: &mut egui::Ui,
    state: &mut CompareTabState,
    live_character: &Character,
    live_output: &Output,
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
        if state.snapshot.is_some() && ui.button("Clear snapshot").clicked() {
            state.snapshot = None;
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
    ui.horizontal(|ui| {
        ui.label("Snapshot:");
        ui.weak(&snap.label);
    });
    // Issue #220 slice 1: surface the passive-tree diff between
    // snapshot and live. Pure data layer — the full in-tree overlay
    // (PoB's `TreeTab.lua:106-125` "compare to spec") is a follow-up
    // slice that needs a multi-spec model.
    let diff = tree_diff(&snap.character.allocated, &live_character.allocated);
    ui.horizontal(|ui| {
        ui.label("Tree diff:");
        let added_color = egui::Color32::from_rgb(0x33, 0xFF, 0x77);
        let removed_color = egui::Color32::from_rgb(0xDD, 0x00, 0x22);
        ui.colored_label(added_color, format!("+{} nodes", diff.added.len()));
        ui.label("/");
        ui.colored_label(removed_color, format!("-{} nodes", diff.removed.len()));
        ui.weak(format!("({} shared) vs snapshot", diff.common.len()));
    });
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
    });
    ui.separator();

    let rows = ordered_diff_rows(
        &snap.output,
        live_output,
        &state.filter,
        state.hide_zero_delta,
        state.sort_mode,
    );

    // Issue #223 follow-up: paste-friendly Markdown export. The button
    // dumps the currently-filtered + sorted rows as a `|`-delimited
    // table; users paste into a build write-up / Discord / GitHub
    // issue to share before-and-after comparisons.
    ui.horizontal(|ui| {
        if ui
            .button("Copy as table")
            .on_hover_text(
                "Copy the filtered + sorted diff rows to the clipboard as a \
                 Markdown table. Paste into a build write-up / GitHub issue.",
            )
            .clicked()
        {
            let md = format_compare_markdown(&rows);
            ui.ctx().copy_text(md);
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
    let mut out = String::new();
    out.push_str("| Stat | Snapshot | Live | Δ | %Δ |\n");
    out.push_str("|---|---:|---:|---:|---:|\n");
    for (key, snap, live) in rows {
        let delta = live - snap;
        let pct = match percent_delta(*snap, *live) {
            Some(p) => format_percent(p),
            None => "—".to_owned(),
        };
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} |\n",
            key,
            format_value(*snap),
            format_value(*live),
            format_delta(delta),
            pct,
        ));
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
        assert!(import_build_text("").is_err());
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
}
