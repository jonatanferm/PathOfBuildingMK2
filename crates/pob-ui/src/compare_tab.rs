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

use eframe::egui;
use pob_engine::{Character, Output};

#[derive(Debug, Clone, Default)]
pub struct CompareTabState {
    pub snapshot: Option<Snapshot>,
    pub filter: String,
    pub hide_zero_delta: bool,
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
}

/// Action the host should take after the user interacted with the
/// Compare tab — currently only "load a comparison build from disk."
/// The host runs the file-pick dialog, imports the build, and runs
/// `compute_full` against it to populate the snapshot Output, then
/// writes the result back via `state.snapshot = Some(...)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompareAction {
    LoadFromFile,
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
            });
        }
        if ui.button("Load comparison from file…").clicked() {
            action = Some(CompareAction::LoadFromFile);
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
    ui.horizontal(|ui| {
        ui.label("Filter:");
        ui.add(
            egui::TextEdit::singleline(&mut state.filter)
                .desired_width(220.0)
                .hint_text("Life, FireResist, MainSkill, …"),
        );
        ui.checkbox(&mut state.hide_zero_delta, "Hide zero deltas");
    });
    ui.separator();

    // Build the diff list: union of keys from both sides, with the
    // delta = live - snapshot. Sort by key for stable order.
    let mut keys: Vec<String> = std::collections::BTreeSet::<String>::from_iter(
        live_output
            .iter()
            .map(|(k, _)| k.to_owned())
            .chain(snap.output.iter().map(|(k, _)| k.to_owned())),
    )
    .into_iter()
    .collect();
    let filter_lc = state.filter.to_ascii_lowercase();
    if !filter_lc.is_empty() {
        keys.retain(|k| k.to_ascii_lowercase().contains(&filter_lc));
    }

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Grid::new("compare_grid")
                .num_columns(4)
                .striped(true)
                .show(ui, |ui| {
                    ui.strong("Stat");
                    ui.strong("Snapshot");
                    ui.strong("Live");
                    ui.strong("Delta");
                    ui.end_row();
                    for key in &keys {
                        let live = live_output.try_get(key).unwrap_or(0.0);
                        let snap_v = snap.output.try_get(key).unwrap_or(0.0);
                        let delta = live - snap_v;
                        if state.hide_zero_delta && delta.abs() < 1e-6 {
                            continue;
                        }
                        ui.label(key);
                        ui.label(format_value(snap_v));
                        ui.label(format_value(live));
                        let color = if delta > 1e-6 {
                            egui::Color32::from_rgb(0x33, 0xFF, 0x77)
                        } else if delta < -1e-6 {
                            egui::Color32::from_rgb(0xDD, 0x00, 0x22)
                        } else {
                            ui.style().visuals.text_color()
                        };
                        ui.colored_label(color, format_delta(delta));
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

fn format_delta(v: f64) -> String {
    if v.abs() < 1e-6 {
        "0".to_owned()
    } else if v > 0.0 {
        format!("+{}", format_value(v))
    } else {
        format_value(v)
    }
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
}
