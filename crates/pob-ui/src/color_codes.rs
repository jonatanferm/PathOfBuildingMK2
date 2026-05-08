//! PoB-style color escape parsing for tooltip / notes / item text.
//!
//! Mirrors upstream PoB's `^N` / `^xRRGGBB` escape convention used in
//! `Data/Global.lua::colorCodes` and the SimpleGraphic `DrawString` path.
//! Item tooltips, gem descriptions, and Notes-tab content all carry
//! these escapes inline; rendering them as plain text loses the visual
//! categorisation (corrupted lines red, crafted lines blue, etc.).
//!
//! Returns an [`egui::text::LayoutJob`] so callers can drop it directly
//! into `ui.label(job)`.

use eframe::egui::{text::LayoutJob, Color32, FontId, TextFormat};

/// Single-digit `^N` palette. PoE convention plus PoB's defaults
/// (white-ish for `^7`, gray for `^8`, etc.). Indices 0..=9.
const PALETTE: [Color32; 10] = [
    Color32::from_rgb(0x00, 0x00, 0x00), // 0 black
    Color32::from_rgb(0xDD, 0x00, 0x22), // 1 red       (NEGATIVE)
    Color32::from_rgb(0x33, 0xFF, 0x77), // 2 green     (POSITIVE)
    Color32::from_rgb(0x70, 0x70, 0xFF), // 3 blue      (WITCH)
    Color32::from_rgb(0xFF, 0xFF, 0x77), // 4 yellow    (RARE)
    Color32::from_rgb(0xCD, 0x22, 0x85), // 5 magenta   (MUTATED)
    Color32::from_rgb(0x88, 0xFF, 0xFF), // 6 cyan      (SOURCE)
    Color32::from_rgb(0xC8, 0xC8, 0xC8), // 7 white     (NORMAL — PoB default)
    Color32::from_rgb(0x80, 0x80, 0x80), // 8 gray      (TIP)
    Color32::from_rgb(0xFF, 0x99, 0x22), // 9 orange    (WARNING)
];

/// Parse `text` and produce a styled `LayoutJob`. Unrecognised escapes
/// fall through unchanged. The default color (no escape) is `default`.
pub fn to_layout_job(text: &str, default: Color32, font: FontId) -> LayoutJob {
    let mut job = LayoutJob::default();
    let mut current_color = default;
    let bytes = text.as_bytes();
    let mut i = 0;
    let mut chunk_start = 0;

    let flush = |job: &mut LayoutJob, slice: &str, color: Color32, font: &FontId| {
        if !slice.is_empty() {
            job.append(slice, 0.0, TextFormat::simple(font.clone(), color));
        }
    };

    while i < bytes.len() {
        if bytes[i] == b'^' {
            // ^xRRGGBB — 8 chars total starting at the caret.
            if let Some(c) = parse_hex_escape(&text[i..]) {
                flush(&mut job, &text[chunk_start..i], current_color, &font);
                current_color = c;
                i += 8;
                chunk_start = i;
                continue;
            }
            // ^N single digit — 2 chars total.
            if let Some(c) = parse_digit_escape(&text[i..]) {
                flush(&mut job, &text[chunk_start..i], current_color, &font);
                current_color = c;
                i += 2;
                chunk_start = i;
                continue;
            }
        }
        i += 1;
    }
    flush(&mut job, &text[chunk_start..], current_color, &font);
    job
}

fn parse_digit_escape(s: &str) -> Option<Color32> {
    let bytes = s.as_bytes();
    if bytes.len() < 2 || bytes[0] != b'^' {
        return None;
    }
    let d = bytes[1];
    if d.is_ascii_digit() {
        Some(PALETTE[(d - b'0') as usize])
    } else {
        None
    }
}

fn parse_hex_escape(s: &str) -> Option<Color32> {
    let bytes = s.as_bytes();
    if bytes.len() < 8 || bytes[0] != b'^' || (bytes[1] != b'x' && bytes[1] != b'X') {
        return None;
    }
    let hex = &s[2..8];
    if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some(Color32::from_rgb(r, g, b))
}

/// Strip any `^N` / `^xRRGGBB` escapes, returning a plain-text view.
/// Useful for clipboard copy paths or the Notes-tab edit-mode buffer.
#[must_use]
// Reserved for future Notes-tab edit-mode buffer (issue #38) — clippy
// flags it as dead code today; it's still tested by the unit suite
// below so don't remove.
#[allow(dead_code)]
pub fn strip_escapes(text: &str) -> String {
    // Slice the original `&str` between matched escapes rather than pushing
    // one byte at a time — pushing `bytes[i] as char` would corrupt any
    // multi-byte UTF-8 sequence (accents, em-dashes, smart quotes, emoji)
    // by mis-mapping continuation bytes to their Latin-1 codepoints. Notes
    // are user-authored free-form text, so non-ASCII is common.
    let mut out = String::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut i = 0;
    let mut chunk_start = 0;
    while i < bytes.len() {
        if bytes[i] == b'^' {
            if parse_hex_escape(&text[i..]).is_some() {
                out.push_str(&text[chunk_start..i]);
                i += 8;
                chunk_start = i;
                continue;
            }
            if parse_digit_escape(&text[i..]).is_some() {
                out.push_str(&text[chunk_start..i]);
                i += 2;
                chunk_start = i;
                continue;
            }
        }
        i += 1;
    }
    out.push_str(&text[chunk_start..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_hex_and_digit_escapes() {
        let s = "^x00FF00green^7white^1red";
        assert_eq!(strip_escapes(s), "greenwhitered");
    }

    #[test]
    fn preserves_non_ascii() {
        // Multi-byte UTF-8 sequences (accents, em-dashes, smart quotes,
        // emoji) must round-trip cleanly through the stripper.
        assert_eq!(strip_escapes("^7café — naïve"), "café — naïve");
        assert_eq!(strip_escapes("^x00FF00fire 🔥 burns"), "fire 🔥 burns");
    }

    #[test]
    fn unrecognised_caret_passes_through() {
        // Lone caret, or `^z` (z is not 0..9 and not x), should remain.
        assert_eq!(strip_escapes("a^zb"), "a^zb");
        assert_eq!(strip_escapes("a^"), "a^");
    }

    #[test]
    fn empty_input_yields_empty_job() {
        let job = to_layout_job("", Color32::WHITE, FontId::default());
        assert!(job.text.is_empty());
    }

    #[test]
    fn layout_job_segments_match_escape_count() {
        // `^x00FF00green^7white` should yield two segments.
        let job = to_layout_job("^x00FF00green^7white", Color32::WHITE, FontId::default());
        // Concatenated text drops the escapes.
        assert_eq!(job.text, "greenwhite");
        // Two style runs — one green, one white.
        assert_eq!(job.sections.len(), 2);
        assert_eq!(job.sections[0].format.color, Color32::from_rgb(0, 0xFF, 0));
        assert_eq!(job.sections[1].format.color, PALETTE[7]);
    }

    #[test]
    fn default_color_used_when_no_leading_escape() {
        let job = to_layout_job("plain text", Color32::RED, FontId::default());
        assert_eq!(job.text, "plain text");
        assert_eq!(job.sections.len(), 1);
        assert_eq!(job.sections[0].format.color, Color32::RED);
    }

    #[test]
    fn pob_unique_item_name_renders_with_embedded_hex() {
        // Upstream PoB stamps unique item names with `^xRRGGBB` to get
        // the orange unique colour. The default fallback should never
        // bleed through when the entire string is wrapped in an escape.
        let job = to_layout_job("^xAF6025Headhunter", Color32::WHITE, FontId::default());
        assert_eq!(job.text, "Headhunter");
        assert_eq!(job.sections.len(), 1);
        assert_eq!(
            job.sections[0].format.color,
            Color32::from_rgb(0xAF, 0x60, 0x25)
        );
    }

    #[test]
    fn gem_description_alternates_between_default_and_palette() {
        // Skill descriptions look like "Deals ^9more^7 damage" — the
        // default colour wraps the prose, the digit escape highlights
        // the keyword. Three sections expected: default, orange, default.
        let job = to_layout_job("Deals ^9more^7 damage", Color32::GRAY, FontId::default());
        assert_eq!(job.text, "Deals more damage");
        assert_eq!(job.sections.len(), 3);
        assert_eq!(job.sections[0].format.color, Color32::GRAY);
        assert_eq!(job.sections[1].format.color, PALETTE[9]);
        assert_eq!(job.sections[2].format.color, PALETTE[7]);
    }

    #[test]
    fn malformed_hex_escape_passes_through_as_literal() {
        // `^xZZZZZZ` has the right shape but invalid hex — the parser
        // should treat the whole sequence as ordinary text, not panic
        // or eat 8 characters silently.
        let job = to_layout_job("a^xZZZZZZb", Color32::WHITE, FontId::default());
        assert_eq!(job.text, "a^xZZZZZZb");
        assert_eq!(job.sections.len(), 1);
    }
}
