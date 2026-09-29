//! The Config tab, driven by Path of Building's own option list
//! (`Modules/ConfigOptions.lua`): every option PoB has, shown or hidden by
//! PoB's own relevance rules for the current build, with values written back
//! into PoB's config inputs.

#![cfg(not(target_arch = "wasm32"))]

use serde_json::Value;

use crate::pob_session::{set_config_command, ConfigOption, PobSession, CONFIG_QUERY};

#[derive(Default)]
pub struct PobConfigState {
    options: Vec<ConfigOption>,
    pending_query: Option<u64>,
    /// Snapshot sequence the options were fetched after.
    fetched_for: Option<u64>,
    search: String,
    show_all: bool,
    error: Option<String>,
    /// Text being edited per numeric/text var (committed on focus loss/Enter).
    edit_buf: ahash::AHashMap<String, String>,
}

fn value_as_bool(v: Option<&Value>) -> bool {
    matches!(v, Some(Value::Bool(true)))
}

fn value_as_text(v: Option<&Value>) -> String {
    match v {
        Some(Value::Number(n)) => {
            let f = n.as_f64().unwrap_or(0.0);
            if f.fract() == 0.0 {
                format!("{}", f as i64)
            } else {
                format!("{f}")
            }
        }
        Some(Value::String(s)) => s.clone(),
        _ => String::new(),
    }
}

/// Render the tab. Returns true if the build changed.
pub fn ui(ui: &mut egui::Ui, state: &mut PobConfigState, pob: &mut PobSession) -> bool {
    // (Re)fetch the option list after each recalculation: relevance depends
    // on the build.
    let snap_seq = pob.snapshot.as_ref().map(|s| s.seq);
    if state.pending_query.is_none() && snap_seq.is_some() && state.fetched_for != snap_seq {
        state.pending_query = Some(pob.query(CONFIG_QUERY));
        state.fetched_for = snap_seq;
    }
    if let Some(seq) = state.pending_query {
        if let Some(res) = pob.take_query(seq) {
            state.pending_query = None;
            match res.and_then(|json| {
                serde_json::from_str::<Vec<ConfigOption>>(&json).map_err(|e| e.to_string())
            }) {
                Ok(opts) => {
                    state.options = opts;
                    state.error = None;
                    state.edit_buf.clear();
                }
                Err(e) => state.error = Some(e),
            }
        }
    }

    ui.horizontal(|ui| {
        ui.label("Search:");
        ui.add(egui::TextEdit::singleline(&mut state.search).desired_width(240.0));
        ui.checkbox(&mut state.show_all, "Show all configurations")
            .on_hover_text("PoB hides options that don't affect this build; show them anyway.");
        if state.pending_query.is_some() || state.options.is_empty() {
            ui.spinner();
        }
    });
    if let Some(e) = &state.error {
        ui.colored_label(egui::Color32::LIGHT_RED, e);
    }
    ui.separator();

    let search = state.search.to_lowercase();
    let visible: Vec<usize> = state
        .options
        .iter()
        .enumerate()
        .filter(|(_, o)| {
            (state.show_all || o.shown)
                && (search.is_empty() || o.label.to_lowercase().contains(&search))
        })
        .map(|(i, _)| i)
        .collect();

    // Group into sections, preserving PoB's order.
    let mut sections: Vec<(String, Vec<usize>)> = Vec::new();
    for i in visible {
        let s = &state.options[i].section;
        match sections.last_mut() {
            Some((name, v)) if name == s => v.push(i),
            _ => sections.push((s.clone(), vec![i])),
        }
    }

    let mut changes: Vec<(String, Value)> = Vec::new();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let cols = ((ui.available_width() / 420.0).floor() as usize).clamp(1, 3);
            ui.columns(cols, |columns| {
                for (idx, (name, items)) in sections.iter().enumerate() {
                    let ui = &mut columns[idx % cols];
                    egui::CollapsingHeader::new(egui::RichText::new(name).strong())
                        .id_salt(("pob_cfg_section", name))
                        .default_open(true)
                        .show(ui, |ui| {
                            egui::Grid::new(("pob_cfg_grid", name))
                                .num_columns(2)
                                .spacing([8.0, 4.0])
                                .show(ui, |ui| {
                                    for &i in items {
                                        if let Some(change) =
                                            option_row(ui, &state.options[i], &mut state.edit_buf)
                                        {
                                            changes.push(change);
                                        }
                                        ui.end_row();
                                    }
                                });
                        });
                }
            });
        });

    let changed = !changes.is_empty();
    for (var, value) in changes {
        // Optimistic local update so the control doesn't snap back while PoB
        // recalculates.
        if let Some(o) = state.options.iter_mut().find(|o| o.var == var) {
            o.value = Some(value.clone());
        }
        pob.command(set_config_command(&var, &value));
    }
    changed
}

fn option_row(
    ui: &mut egui::Ui,
    o: &ConfigOption,
    edit_buf: &mut ahash::AHashMap<String, String>,
) -> Option<(String, Value)> {
    let label = ui.label(&o.label);
    if let Some(tip) = &o.tip {
        label.on_hover_text(tip);
    }
    match o.ty.as_str() {
        "check" => {
            let mut on = value_as_bool(o.value.as_ref());
            if ui.checkbox(&mut on, "").changed() {
                return Some((o.var.clone(), Value::Bool(on)));
            }
        }
        "list" => {
            let current = o
                .value
                .clone()
                .or_else(|| o.opts.first().map(|(v, _)| v.clone()));
            let current_label = o
                .opts
                .iter()
                .find(|(v, _)| {
                    Some(v) == current.as_ref()
                        || v.as_f64()
                            .zip(current.as_ref().and_then(Value::as_f64))
                            .is_some_and(|(a, b)| a == b)
                })
                .map_or_else(String::new, |(_, l)| l.clone());
            let mut picked = None;
            egui::ComboBox::from_id_salt(("pob_cfg", &o.var))
                .selected_text(current_label)
                .width(180.0)
                .show_ui(ui, |ui| {
                    for (v, l) in &o.opts {
                        if ui
                            .selectable_label(Some(v) == current.as_ref(), l)
                            .clicked()
                        {
                            picked = Some(v.clone());
                        }
                    }
                });
            if let Some(v) = picked {
                return Some((o.var.clone(), v));
            }
        }
        "count" | "integer" | "countAllowZero" | "float" | "text" => {
            let buf = edit_buf
                .entry(o.var.clone())
                .or_insert_with(|| value_as_text(o.value.as_ref()));
            let hint = value_as_text(o.placeholder.as_ref());
            let width = if o.ty == "text" { 220.0 } else { 90.0 };
            let resp = ui.add(
                egui::TextEdit::singleline(buf)
                    .hint_text(hint)
                    .desired_width(width),
            );
            let commit = resp.lost_focus()
                || (resp.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
            if commit && *buf != value_as_text(o.value.as_ref()) {
                let v = if o.ty == "text" {
                    Value::String(buf.clone())
                } else if buf.trim().is_empty() {
                    Value::Null
                } else {
                    match buf.trim().parse::<f64>() {
                        Ok(n) => serde_json::Number::from_f64(n).map_or(Value::Null, Value::Number),
                        Err(_) => return None,
                    }
                };
                return Some((o.var.clone(), v));
            }
        }
        _ => {
            ui.weak(value_as_text(o.value.as_ref()));
        }
    }
    None
}
