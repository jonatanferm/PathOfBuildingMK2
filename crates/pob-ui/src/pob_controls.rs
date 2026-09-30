//! Path of Building's own UI controls (dropdowns, edit boxes, checkboxes)
//! mirrored as egui widgets: PoB decides what they contain and whether they
//! show; a change is sent back as the same `SetSel` / `SetText` / toggle a
//! click in PoB would perform.

#![cfg(not(target_arch = "wasm32"))]

use serde_json::Value;

use crate::pob_sidebar::job;

/// One control as serialised by `__mk2_ctl`.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Ctl {
    pub n: String,
    #[serde(default = "yes")]
    pub shown: bool,
    #[serde(default = "yes")]
    pub en: bool,
    pub k: String,
    #[serde(default)]
    pub items: Vec<String>,
    #[serde(default)]
    pub sel: usize,
    #[serde(default)]
    pub buf: String,
    #[serde(default)]
    pub state: bool,
    /// The control's own label, when PoB gives it one.
    #[serde(default)]
    pub label: Option<String>,
    /// Multi-line edit box.
    #[serde(default)]
    pub ml: bool,
    /// Placeholder text for edits.
    #[serde(default)]
    pub prompt: Option<String>,
    /// PoB's width for the control (pixels at PoB's scale).
    #[serde(default)]
    pub w: Option<f32>,
}

fn yes() -> bool {
    true
}

/// Parse a JSON array of controls (empty on error).
#[must_use]
pub fn parse(json: &str) -> Vec<Ctl> {
    serde_json::from_str(json).unwrap_or_default()
}

/// Uncommitted edit-box text, keyed by control id.
#[derive(Default)]
pub struct EditBufs {
    bufs: ahash::AHashMap<String, String>,
}

impl EditBufs {
    /// Drop edits (PoB state changed underneath).
    pub fn clear(&mut self) {
        self.bufs.clear();
    }
}

/// Draw `c`; returns the new value when the user changes it.
pub fn show(ui: &mut egui::Ui, c: &Ctl, id: &str, bufs: &mut EditBufs) -> Option<Value> {
    ui.add_enabled_ui(c.en, |ui| match c.k.as_str() {
        "drop" => {
            let current = c
                .items
                .get(c.sel.saturating_sub(1))
                .cloned()
                .unwrap_or_default();
            let mut picked = None;
            egui::ComboBox::from_id_salt(("pob_ctl", id))
                .selected_text(job(&current, 13.0))
                .width(ui.available_width().min(260.0))
                .truncate()
                .show_ui(ui, |ui| {
                    for (i, item) in c.items.iter().enumerate() {
                        if ui
                            .selectable_label(i + 1 == c.sel, job(item, 13.0))
                            .clicked()
                        {
                            picked = Some(i + 1);
                        }
                    }
                });
            picked
                .filter(|&i| i != c.sel)
                .map(|i| Value::from(i as u64))
        }
        "edit" if c.ml => {
            let buf = bufs
                .bufs
                .entry(id.to_owned())
                .or_insert_with(|| c.buf.clone());
            let resp = ui.add(
                egui::TextEdit::multiline(buf)
                    .hint_text(c.prompt.clone().unwrap_or_default())
                    .desired_width(ui.available_width().min(520.0))
                    .desired_rows(4)
                    .font(egui::TextStyle::Monospace),
            );
            (resp.lost_focus() && *buf != c.buf).then(|| Value::String(buf.clone()))
        }
        "button" => ui
            .button(job(c.label.as_deref().unwrap_or("?"), 13.0))
            .clicked()
            .then_some(Value::Bool(true)),
        "label" => {
            if let Some(l) = &c.label {
                ui.label(job(l, 13.0));
            }
            None
        }
        "edit" => {
            let buf = bufs
                .bufs
                .entry(id.to_owned())
                .or_insert_with(|| c.buf.clone());
            let width = c.w.map_or(60.0, |w| w * 0.85).clamp(36.0, 420.0);
            let resp = ui.add(
                egui::TextEdit::singleline(buf)
                    .hint_text(c.prompt.clone().unwrap_or_default())
                    .desired_width(width.min(ui.available_width().max(36.0))),
            );
            let commit = resp.lost_focus()
                || (resp.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
            (commit && *buf != c.buf).then(|| Value::String(buf.clone()))
        }
        "check" => {
            let mut on = c.state;
            let label = c
                .label
                .as_deref()
                .map(|l| job(l.trim_end_matches(':'), 13.0));
            ui.checkbox(&mut on, label.unwrap_or_default())
                .changed()
                .then_some(Value::Bool(on))
        }
        _ => None,
    })
    .inner
}

/// Lua that applies `value` to the control at Lua expression `ctl`.
#[must_use]
pub fn set_command(ctl: &str, value: &Value) -> String {
    let v = match value {
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => crate::pob_session::lua_str(s),
        _ => "nil".to_owned(),
    };
    format!("__mk2_set_ctl({ctl}, {v})\n")
}

/// One PoB control at a fixed Lua path, refreshed after each recalculation
/// (e.g. the Import/Export tab's "Export support" checkbox).
#[cfg(not(target_arch = "wasm32"))]
pub struct Single {
    path: &'static str,
    ctl: Option<Ctl>,
    pending: Option<u64>,
    fetched_for: Option<u64>,
    bufs: EditBufs,
}

#[cfg(not(target_arch = "wasm32"))]
impl Single {
    #[must_use]
    pub fn new(path: &'static str) -> Self {
        Self {
            path,
            ctl: None,
            pending: None,
            fetched_for: None,
            bufs: EditBufs::default(),
        }
    }

    /// Draw the control (if PoB has answered); changes go straight to PoB.
    pub fn ui(&mut self, ui: &mut egui::Ui, pob: &mut crate::pob_session::PobSession) {
        let snap = pob.snapshot.as_ref().map(|s| s.seq);
        if self.pending.is_none() && snap.is_some() && self.fetched_for != snap {
            self.pending = Some(pob.query(&format!(
                "local c = {p}\nreturn c and __mk2_json(__mk2_ctl('c', c)) or 'null'",
                p = self.path
            )));
            self.fetched_for = snap;
        }
        if let Some(seq) = self.pending {
            if let Some(res) = pob.take_query(seq) {
                self.pending = None;
                self.ctl = res.ok().and_then(|j| serde_json::from_str(&j).ok());
                self.bufs.clear();
            }
        }
        if let Some(c) = self.ctl.as_ref().filter(|c| c.shown) {
            if let Some(v) = show(ui, c, self.path, &mut self.bufs) {
                pob.command(set_command(self.path, &v));
            }
        }
    }
}
