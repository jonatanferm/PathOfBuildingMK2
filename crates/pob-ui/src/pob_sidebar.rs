//! Path of Building's own sidebar stat list (its `statBox`), rendered with
//! PoB's colour codes.

#[cfg(not(target_arch = "wasm32"))]
pub use pob_calc::worker::SidebarLine as Line;

/// Stand-in on wasm (no PoB backend there yet).
#[cfg(target_arch = "wasm32")]
#[derive(Debug, Clone, Default)]
pub struct Line {
    pub label: String,
    pub value: String,
    pub height: f32,
}

/// PoB's `^0`–`^9` palette (SimpleGraphic).
fn digit_color(d: u8) -> egui::Color32 {
    match d {
        b'0' => egui::Color32::BLACK,
        b'1' => egui::Color32::from_rgb(255, 0, 0),
        b'2' => egui::Color32::from_rgb(0, 255, 0),
        b'3' => egui::Color32::from_rgb(0, 0, 255),
        b'4' => egui::Color32::from_rgb(255, 255, 0),
        b'5' => egui::Color32::from_rgb(255, 0, 255),
        b'6' => egui::Color32::from_rgb(0, 255, 255),
        b'8' => egui::Color32::from_gray(178),
        b'9' => egui::Color32::from_gray(102),
        _ => egui::Color32::from_gray(230),
    }
}

/// Split a PoB-coloured string (`^7`, `^xRRGGBB`) into coloured runs.
#[must_use]
pub fn runs(s: &str, default: egui::Color32) -> Vec<(String, egui::Color32)> {
    let b = s.as_bytes();
    let mut out: Vec<(String, egui::Color32)> = Vec::new();
    let mut color = default;
    let mut cur = String::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'^' && i + 1 < b.len() {
            if b[i + 1].is_ascii_digit() {
                if !cur.is_empty() {
                    out.push((std::mem::take(&mut cur), color));
                }
                color = digit_color(b[i + 1]);
                i += 2;
                continue;
            }
            if (b[i + 1] == b'x' || b[i + 1] == b'X') && i + 8 <= b.len() {
                if let Ok(rgb) = u32::from_str_radix(&s[i + 2..i + 8], 16) {
                    if !cur.is_empty() {
                        out.push((std::mem::take(&mut cur), color));
                    }
                    color = egui::Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8);
                    i += 8;
                    continue;
                }
            }
        }
        let ch = s[i..].chars().next().unwrap_or(' ');
        cur.push(ch);
        i += ch.len_utf8();
    }
    if !cur.is_empty() {
        out.push((cur, color));
    }
    out
}

/// A `LayoutJob` for a PoB-coloured string.
#[must_use]
pub fn job(s: &str, size: f32) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    for (text, color) in runs(s, egui::Color32::from_gray(230)) {
        job.append(
            &text,
            0.0,
            egui::TextFormat {
                font_id: egui::FontId::proportional(size),
                color,
                ..Default::default()
            },
        );
    }
    job
}

/// Render the sidebar lines and warnings.
pub fn render(ui: &mut egui::Ui, lines: &[Line], warnings: &[String]) {
    for w in warnings {
        ui.label(job(w, 13.0));
    }
    egui::Grid::new("pob_sidebar")
        .num_columns(2)
        .spacing([6.0, 2.0])
        .show(ui, |ui| {
            for line in lines {
                if line.label.trim().is_empty() && line.value.trim().is_empty() {
                    ui.add_space(line.height.min(8.0));
                    ui.end_row();
                    continue;
                }
                ui.label(job(&line.label, 14.0));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(job(&line.value, 14.0));
                });
                ui.end_row();
            }
        });
}
