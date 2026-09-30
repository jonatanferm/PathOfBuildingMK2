//! The Calcs tab, as Path of Building lays it out: its section cards,
//! rows shown for the current skill, cells formatted by PoB, and PoB's own
//! breakdown for any clicked cell.

#![cfg(not(target_arch = "wasm32"))]

use crate::pob_session::{breakdown_query, BreakdownSection, CalcSection, PobSession, CALCS_QUERY};
use crate::pob_sidebar::job;

#[derive(Default)]
pub struct PobCalcsState {
    sections: Vec<CalcSection>,
    pending: Option<u64>,
    fetched_for: Option<u64>,
    error: Option<String>,
    selected: Option<[u32; 4]>,
    breakdown: Vec<BreakdownSection>,
    pending_breakdown: Option<u64>,
    search: String,
    edit_bufs: crate::pob_controls::EditBufs,
}

fn colour(code: Option<&str>) -> egui::Color32 {
    code.and_then(|c| {
        crate::pob_sidebar::runs(&format!("{c}x"), egui::Color32::GRAY)
            .first()
            .map(|r| r.1)
    })
    .unwrap_or(egui::Color32::GRAY)
}

pub fn ui(ui: &mut egui::Ui, state: &mut PobCalcsState, pob: &mut PobSession) {
    let snap_seq = pob.snapshot.as_ref().map(|s| s.seq);
    if state.pending.is_none() && snap_seq.is_some() && state.fetched_for != snap_seq {
        state.pending = Some(pob.query(CALCS_QUERY));
        state.fetched_for = snap_seq;
        // Refresh the open breakdown too.
        if let Some(r) = state.selected {
            state.pending_breakdown = Some(pob.query(&breakdown_query(r)));
        }
    }
    if let Some(seq) = state.pending {
        if let Some(res) = pob.take_query(seq) {
            state.pending = None;
            match res.and_then(|j| {
                serde_json::from_str::<Vec<CalcSection>>(&j).map_err(|e| e.to_string())
            }) {
                Ok(s) => {
                    state.sections = s;
                    state.error = None;
                    state.edit_bufs.clear();
                }
                Err(e) => state.error = Some(e),
            }
        }
    }
    if let Some(seq) = state.pending_breakdown {
        if let Some(res) = pob.take_query(seq) {
            state.pending_breakdown = None;
            state.breakdown = res
                .ok()
                .and_then(|j| serde_json::from_str::<Vec<BreakdownSection>>(&j).ok())
                .unwrap_or_default();
        }
    }

    ui.horizontal(|ui| {
        ui.label("Search:");
        ui.add(egui::TextEdit::singleline(&mut state.search).desired_width(220.0));
        if state.pending.is_some() {
            ui.spinner();
        }
    });
    if let Some(e) = &state.error {
        ui.colored_label(egui::Color32::LIGHT_RED, e);
    }
    ui.separator();

    let search = state.search.to_lowercase();
    let mut clicked: Option<[u32; 4]> = None;
    let mut control_change: Option<(u32, String, serde_json::Value)> = None;
    let breakdown_width = if state.breakdown.is_empty() {
        0.0
    } else {
        420.0
    };
    ui.horizontal_top(|ui| {
        let width = (ui.available_width() - breakdown_width).max(300.0);
        ui.allocate_ui(egui::vec2(width, ui.available_height()), |ui| {
            egui::ScrollArea::vertical()
                .id_salt("pob_calcs_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let cols = ((ui.available_width() / 380.0).floor() as usize).clamp(1, 3);
                    ui.columns(cols, |columns| {
                        for (i, sec) in state.sections.iter().enumerate() {
                            let column = sec
                                .g
                                .map_or(i % cols, |g| (g as usize).saturating_sub(1) % cols);
                            let ui = &mut columns[column];
                            let frame = egui::Frame::group(ui.style())
                                .stroke(egui::Stroke::new(1.5, colour(sec.col.as_deref())));
                            frame.show(ui, |ui| {
                                for sub in &sec.subs {
                                    let visible_rows: Vec<_> = sub
                                        .rows
                                        .iter()
                                        .filter(|r| {
                                            search.is_empty()
                                                || r.l.to_lowercase().contains(&search)
                                                || sub.l.to_lowercase().contains(&search)
                                        })
                                        .collect();
                                    if visible_rows.is_empty() && !search.is_empty() {
                                        continue;
                                    }
                                    ui.horizontal(|ui| {
                                        ui.label(egui::RichText::new(&sub.l).strong());
                                        if let Some(x) = &sub.x {
                                            ui.label(job(x, 12.0));
                                        }
                                    });
                                    egui::Grid::new(("pob_calc_sub", sec.id.clone(), &sub.l))
                                        .num_columns(3)
                                        .spacing([6.0, 1.0])
                                        .show(ui, |ui| {
                                            for row in visible_rows {
                                                let colon = if row.l.is_empty() { "" } else { ":" };
                                                ui.label(job(
                                                    &format!(
                                                        "{}{}{colon}",
                                                        row.c.as_deref().unwrap_or("^7"),
                                                        row.l
                                                    ),
                                                    13.0,
                                                ));
                                                for cell in &row.cells {
                                                    if let Some(ctl) = &cell.ctl {
                                                        let id =
                                                            format!("calc{}_{}", cell.r[0], ctl.n);
                                                        if let Some(v) = crate::pob_controls::show(
                                                            ui,
                                                            ctl,
                                                            &id,
                                                            &mut state.edit_bufs,
                                                        ) {
                                                            control_change =
                                                                Some((cell.r[0], ctl.n.clone(), v));
                                                        }
                                                        continue;
                                                    }
                                                    let selected = state.selected == Some(cell.r);
                                                    let resp = ui.add(
                                                        egui::Button::new(job(&cell.t, 13.0))
                                                            .frame(selected)
                                                            .small(),
                                                    );
                                                    if cell.b && resp.clicked() {
                                                        clicked = Some(cell.r);
                                                    }
                                                }
                                                ui.end_row();
                                            }
                                        });
                                    ui.add_space(4.0);
                                }
                            });
                            ui.add_space(6.0);
                        }
                    });
                });
        });
        if !state.breakdown.is_empty() {
            ui.separator();
            ui.vertical(|ui| {
                ui.set_width(breakdown_width - 12.0);
                ui.horizontal(|ui| {
                    ui.heading("Breakdown");
                    if ui.small_button("✕").clicked() {
                        state.selected = None;
                        state.breakdown.clear();
                    }
                });
                egui::ScrollArea::vertical()
                    .id_salt("pob_breakdown_scroll")
                    .show(ui, |ui| {
                        for (i, s) in state.breakdown.iter().enumerate() {
                            match s.ty.as_str() {
                                "TEXT" => {
                                    for l in &s.lines {
                                        ui.label(job(l, 13.0));
                                    }
                                }
                                "TABLE" => {
                                    if let Some(label) = &s.label {
                                        ui.label(egui::RichText::new(label).strong());
                                    }
                                    egui::Grid::new(("pob_bd_table", i))
                                        .striped(true)
                                        .spacing([8.0, 1.0])
                                        .show(ui, |ui| {
                                            for c in &s.cols {
                                                ui.label(
                                                    egui::RichText::new(
                                                        c.label.clone().unwrap_or_default(),
                                                    )
                                                    .strong(),
                                                );
                                            }
                                            ui.end_row();
                                            for r in &s.rows {
                                                for v in r {
                                                    ui.label(job(v, 12.0));
                                                }
                                                ui.end_row();
                                            }
                                        });
                                    if let Some(f) = &s.footer {
                                        ui.label(job(f, 12.0));
                                    }
                                }
                                _ => {}
                            }
                            ui.add_space(6.0);
                        }
                    });
            });
        }
    });
    if let Some((section, name, value)) = control_change {
        pob.command(crate::pob_controls::set_command(
            &format!(
                "build.calcsTab.sectionList[{section}].controls[{}]",
                crate::pob_session::lua_str(&name)
            ),
            &value,
        ));
    }
    if let Some(r) = clicked {
        state.selected = Some(r);
        state.pending_breakdown = Some(pob.query(&breakdown_query(r)));
    }
}
