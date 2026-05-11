//! Notes tab — free-form text editor with PoB-style color escape rendering.

use eframe::egui;

use crate::color_codes;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NotesMode {
    #[default]
    Edit,
    Render,
}

#[derive(Debug, Clone, Default)]
pub struct NotesTabState {
    pub mode: NotesMode,
}

/// Issue #225: PoB's `NotesTab.lua:21-32` color-code toolbar. Each
/// entry pairs a button label with the escape sequence the button
/// inserts at the cursor when clicked. The label intentionally
/// mirrors PoB's naming (NORMAL / MAGIC / RARE / UNIQUE / FIRE / …)
/// so users coming from PoB find the same options.
///
/// The escape sequences match `color_codes::PALETTE` for single-digit
/// codes and use `^x…` for named colours that need a specific hex
/// (RARE, UNIQUE, FIRE, COLD, LIGHTNING, CHAOS, attribute trio) per
/// upstream `Data/Global.lua::colorCodes`.
pub const NOTE_COLOR_BUTTONS: &[(&str, &str)] = &[
    ("Normal", "^7"),
    ("Magic", "^x8888FF"),
    ("Rare", "^xFFFF77"),
    ("Unique", "^xAF6025"),
    ("Fire", "^xB97123"),
    ("Cold", "^x3F6DB3"),
    ("Lightning", "^xADAA47"),
    ("Chaos", "^xD02090"),
    ("Strength", "^xD02020"),
    ("Dexterity", "^x20D020"),
    ("Intelligence", "^x6060FF"),
    ("Default", "^7"),
];

/// Pure helper: insert `escape` at the cursor's byte index inside
/// `text`. Returns `(new_text, new_cursor_byte_index)` so the
/// `TextEditState` can be repositioned after the insertion.
///
/// Issue #225: pulled out of the egui loop so the cursor math has a
/// unit-test home. `cursor_byte` is clamped to the valid char-boundary
/// range — out-of-range values (a stale cursor index after the user
/// shrunk the buffer) snap to the end so the insert still lands
/// somewhere meaningful instead of panicking.
#[must_use]
pub fn insert_color_escape(text: &str, cursor_byte: usize, escape: &str) -> (String, usize) {
    let clamped = clamp_to_char_boundary(text, cursor_byte);
    let mut out = String::with_capacity(text.len() + escape.len());
    out.push_str(&text[..clamped]);
    out.push_str(escape);
    out.push_str(&text[clamped..]);
    (out, clamped + escape.len())
}

/// Clamp `byte_idx` to the nearest preceding char boundary inside
/// `s`. Out-of-range indices snap to `s.len()`. Pulled out so the
/// branch matrix for the insert helper stays small.
fn clamp_to_char_boundary(s: &str, byte_idx: usize) -> usize {
    if byte_idx >= s.len() {
        return s.len();
    }
    let mut idx = byte_idx;
    while idx > 0 && !s.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

pub fn ui(ui: &mut egui::Ui, notes: &mut String, state: &mut NotesTabState) {
    ui.horizontal(|ui| {
        ui.heading("Notes");
        ui.separator();
        ui.selectable_value(&mut state.mode, NotesMode::Edit, "Edit");
        ui.selectable_value(&mut state.mode, NotesMode::Render, "Render");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let chars = notes.chars().count();
            let lines = notes
                .lines()
                .count()
                .max(if notes.is_empty() { 0 } else { 1 });
            let words = notes.split_whitespace().count();
            ui.weak(format!("{chars} chars · {words} words · {lines} lines"));
        });
    });
    ui.separator();

    // Issue #225: PoB-style color-code toolbar above the editor. The
    // buttons paint their own label in the colour they emit so the
    // palette is self-documenting — a user shopping for "the green
    // one" can scan the row visually without remembering "^2 means
    // green".
    let edit_id = egui::Id::new("notes-tab-edit");
    if matches!(state.mode, NotesMode::Edit) {
        ui.horizontal_wrapped(|ui| {
            ui.label("Color:");
            let font = egui::TextStyle::Button.resolve(ui.style());
            for (label, escape) in NOTE_COLOR_BUTTONS {
                let preview = color_codes::to_layout_job(
                    &format!("{escape}{label}"),
                    ui.style().visuals.text_color(),
                    font.clone(),
                );
                if ui
                    .small_button(preview)
                    .on_hover_text(format!("Insert `{escape}` at the cursor"))
                    .clicked()
                {
                    // Pull the cursor byte index from the egui
                    // TextEditState; fall back to "end of buffer" when
                    // the editor hasn't been focused yet (cursor state
                    // is `None`).
                    let cursor_byte = egui::TextEdit::load_state(ui.ctx(), edit_id)
                        .and_then(|st| st.cursor.char_range())
                        .map(|range| {
                            let primary = range.primary.index;
                            // Cursor index from egui is a char count;
                            // convert to a byte index by walking the
                            // string. Both are zero for an empty
                            // buffer, so the fall-back of "end" lands
                            // correctly even for the first insert.
                            notes
                                .char_indices()
                                .nth(primary)
                                .map_or(notes.len(), |(b, _)| b)
                        })
                        .unwrap_or_else(|| notes.len());
                    let (new_text, new_cursor) = insert_color_escape(notes, cursor_byte, escape);
                    *notes = new_text;
                    // Reposition the cursor so subsequent typing
                    // appends *after* the escape rather than before
                    // it. Translate the byte index back to a char
                    // index for egui's CCursor.
                    let new_char_idx = notes
                        .char_indices()
                        .position(|(b, _)| b == new_cursor)
                        .unwrap_or_else(|| notes.chars().count());
                    if let Some(mut st) = egui::TextEdit::load_state(ui.ctx(), edit_id) {
                        let ccursor = egui::text::CCursor::new(new_char_idx);
                        st.cursor
                            .set_char_range(Some(egui::text::CCursorRange::one(ccursor)));
                        st.store(ui.ctx(), edit_id);
                    }
                }
            }
        });
        ui.add_space(4.0);
    }

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| match state.mode {
            NotesMode::Edit => {
                ui.add(
                    egui::TextEdit::multiline(notes)
                        .id(edit_id)
                        .desired_width(f32::INFINITY)
                        .desired_rows(40)
                        .font(egui::TextStyle::Body),
                );
            }
            NotesMode::Render => {
                let default_color = ui.style().visuals.text_color();
                let font = egui::TextStyle::Body.resolve(ui.style());
                let job = color_codes::to_layout_job(notes, default_color, font);
                ui.label(job);
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_at_start_prepends_escape() {
        let (out, cur) = insert_color_escape("hello", 0, "^7");
        assert_eq!(out, "^7hello");
        assert_eq!(cur, 2);
    }

    #[test]
    fn insert_in_middle_splits_text_and_advances_cursor() {
        // Cursor sits between `hel` and `lo`.
        let (out, cur) = insert_color_escape("hello", 3, "^x00FF00");
        assert_eq!(out, "hel^x00FF00lo");
        // Cursor now sits right after the escape — typing continues
        // inside the coloured span.
        assert_eq!(cur, 3 + "^x00FF00".len());
    }

    #[test]
    fn insert_at_end_appends_escape() {
        let (out, cur) = insert_color_escape("hello", 5, "^9");
        assert_eq!(out, "hello^9");
        assert_eq!(cur, 7);
    }

    #[test]
    fn cursor_beyond_buffer_snaps_to_end() {
        // Stale cursor index (the user shrunk the buffer between
        // frames). The insert should still land somewhere
        // sensible — the end of the current text — rather than
        // panicking on the slice index.
        let (out, cur) = insert_color_escape("hi", 999, "^7");
        assert_eq!(out, "hi^7");
        assert_eq!(cur, 4);
    }

    #[test]
    fn cursor_inside_multibyte_char_snaps_to_preceding_boundary() {
        // `é` is two bytes in UTF-8; an interior byte index (1) is
        // not a char boundary. The insert must clamp to byte 0 so
        // the slice operation is valid, not panic mid-codepoint.
        let s = "é!";
        let (out, cur) = insert_color_escape(s, 1, "^9");
        // Insert lands before the multi-byte char.
        assert_eq!(out, "^9é!");
        assert_eq!(cur, 2);
    }

    #[test]
    fn toolbar_button_list_covers_pob_palette() {
        // Smoke: the PoB toolbar exposes 12 buttons (NORMAL plus the
        // 10 named colours plus DEFAULT). The Rust mirror should
        // match so the user gets the same options. Catches accidental
        // removal of an entry when someone reformats the constant.
        assert_eq!(NOTE_COLOR_BUTTONS.len(), 12);
        let labels: Vec<&str> = NOTE_COLOR_BUTTONS.iter().map(|(l, _)| *l).collect();
        for required in [
            "Normal",
            "Magic",
            "Rare",
            "Unique",
            "Fire",
            "Cold",
            "Lightning",
            "Chaos",
            "Strength",
            "Dexterity",
            "Intelligence",
            "Default",
        ] {
            assert!(
                labels.contains(&required),
                "missing toolbar button: {required}"
            );
        }
        // Every escape is a syntactically valid PoB color code
        // (`^N` single digit or `^xRRGGBB`).
        for (label, escape) in NOTE_COLOR_BUTTONS {
            assert!(escape.starts_with('^'), "{label} escape missing caret");
            assert!(
                escape.len() == 2 || (escape.len() == 8 && escape.as_bytes()[1] == b'x'),
                "{label} escape `{escape}` is not `^N` or `^xRRGGBB`",
            );
        }
    }
}
