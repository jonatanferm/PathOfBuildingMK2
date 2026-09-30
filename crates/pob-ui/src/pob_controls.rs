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
        "edit" => {
            let buf = bufs
                .bufs
                .entry(id.to_owned())
                .or_insert_with(|| c.buf.clone());
            let resp = ui.add(
                egui::TextEdit::singleline(buf)
                    .desired_width(ui.available_width().clamp(36.0, 60.0)),
            );
            let commit = resp.lost_focus()
                || (resp.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
            (commit && *buf != c.buf).then(|| Value::String(buf.clone()))
        }
        "check" => {
            let mut on = c.state;
            ui.checkbox(&mut on, "")
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
