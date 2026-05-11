//! Inline socket renderer for the Items tab.
//!
//! Issue #221 (slice 1): consume the `parse_socket_string` /
//! `SocketColor` / `SocketGroup` helpers from PR #381 and paint each
//! socket as a coloured dot inside the equipped-item card. Sockets
//! inside the same `SocketGroup` are connected by a short link bar;
//! gaps between groups are larger so the eye can tell groups apart.
//!
//! The painting is tiny — the interesting bit is the layout helper
//! [`socket_render_layout`], which is pure and unit-tested. The egui
//! drawing function ([`draw_sockets`]) just walks that layout.
//!
//! Issue #221 follow-up: click-to-cycle is wired via [`draw_sockets`]'s
//! `SocketsResponse::clicked_dot` and the pure [`cycle_socket_color`] /
//! [`apply_socket_cycle_at`] helpers. Clicking a dot advances its
//! colour through `R → G → B → W → R`; abyss sockets stay abyss
//! (`SocketColor::cycle_next` is a no-op on abyss).

use eframe::egui;
use pob_data::{SocketColor, SocketGroup};

/// Visual size constants. Pulled out of the layout fn so tests can
/// reason about the geometry without poking at egui defaults.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SocketLayoutConfig {
    /// Diameter of one socket dot, in points.
    pub dot_diameter: f32,
    /// Horizontal gap between two linked sockets in the same group.
    pub link_gap: f32,
    /// Horizontal gap between two separate groups (no link bar).
    pub group_gap: f32,
    /// Thickness of the link bar drawn between two linked sockets.
    pub link_thickness: f32,
}

impl Default for SocketLayoutConfig {
    fn default() -> Self {
        // Tuned by eye against PoB's classic socket panel — small
        // enough to fit on a single line in the item card, large
        // enough that the colours read at a glance.
        Self {
            dot_diameter: 12.0,
            link_gap: 4.0,
            group_gap: 8.0,
            link_thickness: 2.5,
        }
    }
}

/// One drawable element in the laid-out socket row.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RenderItem {
    /// A coloured socket dot at the given x-centre, with the supplied colour.
    Dot { x_center: f32, color: SocketColor },
    /// A short horizontal link bar joining two adjacent linked sockets.
    /// `x_start..x_end` is the bar's x-range; it sits centred on the row.
    Link { x_start: f32, x_end: f32 },
}

/// Issue #221 follow-up: one hit zone for click-to-toggle-link. Emitted
/// for every adjacent pair of sockets (within a group → `linked = true`,
/// across groups → `linked = false`). The renderer turns each zone into
/// a screen rect and reports `clicked_link` when the user clicks it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GapZone {
    pub x_start: f32,
    pub x_end: f32,
    pub linked: bool,
}

/// Result of laying out a list of socket groups in a horizontal row.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SocketLayout {
    pub items: Vec<RenderItem>,
    /// Issue #221 follow-up: per-pair hit zones for the link-toggle UI.
    /// Indexed left-to-right; entry `n` sits between dot `n` and `n+1`.
    pub gap_zones: Vec<GapZone>,
    /// Total width consumed, in points. `0.0` for an empty input.
    pub width: f32,
}

/// Compute draw positions for a sequence of socket groups.
///
/// The row is anchored at `x = 0.0` and grows to the right. Within a
/// group, consecutive dots are separated by `cfg.link_gap` and joined
/// by a `Link` bar that spans the gap. Between groups, the row jumps
/// by `cfg.group_gap` with no link bar — that visual break is what
/// communicates "these sockets are not linked".
///
/// Empty groups (which `parse_socket_string` never produces, but a
/// caller could synthesise) are silently skipped.
pub fn socket_render_layout(groups: &[SocketGroup], cfg: SocketLayoutConfig) -> SocketLayout {
    let mut items = Vec::new();
    let mut gap_zones = Vec::new();
    let mut x = 0.0_f32;
    let radius = cfg.dot_diameter * 0.5;
    let mut last_dot_right: Option<f32> = None;
    for group in groups {
        if group.is_empty() {
            continue;
        }
        if let Some(prev_right) = last_dot_right {
            let zone_start = prev_right;
            x += cfg.group_gap;
            gap_zones.push(GapZone {
                x_start: zone_start,
                x_end: x,
                linked: false,
            });
        }
        for (i, color) in group.colors.iter().enumerate() {
            if i > 0 {
                // Link bar fills the gap between previous dot edge and
                // next dot edge — same gap on both sides of the bar.
                let bar_start = x;
                x += cfg.link_gap;
                items.push(RenderItem::Link {
                    x_start: bar_start,
                    x_end: x,
                });
                gap_zones.push(GapZone {
                    x_start: bar_start,
                    x_end: x,
                    linked: true,
                });
            }
            x += radius;
            items.push(RenderItem::Dot {
                x_center: x,
                color: *color,
            });
            x += radius;
            last_dot_right = Some(x);
        }
    }
    SocketLayout {
        items,
        gap_zones,
        width: x,
    }
}

/// Map a `SocketColor` to an on-screen fill colour. Tuned to match
/// PoB's traditional palette (saturated red/green/blue, near-white,
/// dim grey for abyss).
pub fn socket_fill(color: SocketColor) -> egui::Color32 {
    match color {
        SocketColor::Red => egui::Color32::from_rgb(220, 60, 60),
        SocketColor::Green => egui::Color32::from_rgb(60, 200, 90),
        SocketColor::Blue => egui::Color32::from_rgb(80, 120, 240),
        SocketColor::White => egui::Color32::from_rgb(230, 230, 230),
        SocketColor::Abyss => egui::Color32::from_rgb(70, 70, 70),
    }
}

/// Issue #221 follow-up: result of drawing + hit-testing a socket row.
/// The renderer reports the 0-based index of the dot the user clicked,
/// counted across every group in left-to-right order (matching the
/// order [`socket_render_layout`] emits `RenderItem::Dot` elements).
/// `None` when no dot was clicked this frame. Carries through egui's
/// own [`egui::Response`] so call sites can still wire up
/// `on_hover_ui` / focus-style affordances.
#[derive(Debug)]
pub struct SocketsResponse {
    pub response: egui::Response,
    pub clicked_dot: Option<usize>,
    /// Issue #221 follow-up: 0-based index of the link gap the user
    /// clicked this frame, if any. Counts both currently-linked and
    /// currently-unlinked gaps, left-to-right (same order as
    /// [`SocketLayout::gap_zones`]). Dot clicks take precedence — a
    /// click that lands on a dot reports `clicked_dot` and leaves
    /// `clicked_link` as `None`.
    pub clicked_link: Option<usize>,
}

/// Issue #221 follow-up: cycle the colour at `dot_index` of `sockets`
/// to the next colour in the `R → G → B → W → R` ring (abyss stays
/// abyss). Pure: doesn't touch egui or item state. Returns the new
/// socket string with the rest of the layout (link bars, group
/// separators) preserved verbatim.
///
/// `dot_index` is 0-based and counts only **socket characters** (the
/// same order [`draw_sockets`] paints them in), not raw string
/// indices. An index past the last socket returns the original string
/// unchanged — defensive against a stale click after the user just
/// edited the socket string by hand.
#[must_use]
pub fn apply_socket_cycle_at(sockets: &str, dot_index: usize) -> String {
    let mut out = String::with_capacity(sockets.len());
    let mut seen = 0usize;
    let mut applied = false;
    for c in sockets.chars() {
        if let Some(col) = SocketColor::from_letter(c) {
            if !applied && seen == dot_index {
                out.push(col.cycle_next().letter());
                applied = true;
            } else {
                out.push(c);
            }
            seen += 1;
        } else {
            // Separator (`-`, ` `, etc.) — pass through unchanged.
            out.push(c);
        }
    }
    out
}

/// Issue #221 follow-up: toggle the link separator at the `link_index`-th
/// gap between adjacent sockets in `sockets`. A currently-linked gap
/// (`-`) flips to unlinked (` `), and vice versa. Pure: no egui, no
/// item state. Gaps are counted left-to-right matching the order
/// [`SocketLayout::gap_zones`] is emitted.
///
/// Returns the original string unchanged when:
/// - `link_index` is past the last gap (defensive against stale clicks),
/// - either socket adjacent to the gap is abyss (jewel sockets can't be
///   part of a link group, so creating or breaking a link there would
///   produce an invalid socket string).
#[must_use]
pub fn apply_socket_link_toggle_at(sockets: &str, link_index: usize) -> String {
    let chars: Vec<char> = sockets.chars().collect();
    // Collect (start, end) char-index ranges for each separator run
    // that sits *between* two sockets — leading and trailing whitespace
    // are skipped so they can't be addressed by a click.
    let mut sep_runs: Vec<(usize, usize)> = Vec::new();
    let mut run_start: Option<usize> = None;
    let mut seen_socket = false;
    for (i, &c) in chars.iter().enumerate() {
        let is_sock = SocketColor::from_letter(c).is_some();
        if is_sock {
            if let Some(rs) = run_start {
                if seen_socket {
                    sep_runs.push((rs, i));
                }
                run_start = None;
            }
            seen_socket = true;
        } else if run_start.is_none() {
            run_start = Some(i);
        }
    }
    let Some(&(rs, re)) = sep_runs.get(link_index) else {
        return sockets.to_string();
    };
    // Abyss check: the socket immediately before the run is the last
    // socket character in `chars[..rs]`; the one after is the first
    // socket character in `chars[re..]`.
    let left = chars[..rs]
        .iter()
        .rev()
        .find_map(|c| SocketColor::from_letter(*c));
    let right = chars[re..]
        .iter()
        .find_map(|c| SocketColor::from_letter(*c));
    if left == Some(SocketColor::Abyss) || right == Some(SocketColor::Abyss) {
        return sockets.to_string();
    }
    let run: String = chars[rs..re].iter().collect();
    let new_sep = if run.contains('-') { " " } else { "-" };
    let mut out = String::with_capacity(sockets.len());
    out.extend(chars[..rs].iter().copied());
    out.push_str(new_sep);
    out.extend(chars[re..].iter().copied());
    out
}

/// Paint the supplied socket groups inline into `ui`. Allocates a
/// horizontal slot of exactly the layout width × dot height and draws
/// the dots + link bars into it. Returns a [`SocketsResponse`] so call
/// sites can react to a click on a specific dot.
pub fn draw_sockets(
    ui: &mut egui::Ui,
    groups: &[SocketGroup],
    cfg: SocketLayoutConfig,
) -> SocketsResponse {
    let layout = socket_render_layout(groups, cfg);
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(layout.width.max(1.0), cfg.dot_diameter),
        egui::Sense::click(),
    );
    let painter = ui.painter_at(rect);
    let radius = cfg.dot_diameter * 0.5;
    let cy = rect.center().y;
    let stroke_color = ui.visuals().widgets.noninteractive.bg_stroke.color;
    let link_color = ui.visuals().widgets.noninteractive.fg_stroke.color;
    // Walk the layout once: paint each item, and on each `Dot` build
    // up the (dot_index, screen_center, radius) hit-test table so we
    // can map a click pointer back to a dot index after the
    // `interact()` call resolves below.
    let mut dot_centers: Vec<egui::Pos2> = Vec::new();
    for item in &layout.items {
        match item {
            RenderItem::Dot { x_center, color } => {
                let center = egui::pos2(rect.left() + *x_center, cy);
                painter.circle_filled(center, radius, socket_fill(*color));
                // Thin outline so light dots (white) read on a light bg.
                painter.circle_stroke(center, radius, egui::Stroke::new(1.0, stroke_color));
                dot_centers.push(center);
            }
            RenderItem::Link { x_start, x_end } => {
                let bar = egui::Rect::from_min_max(
                    egui::pos2(rect.left() + *x_start, cy - cfg.link_thickness * 0.5),
                    egui::pos2(rect.left() + *x_end, cy + cfg.link_thickness * 0.5),
                );
                painter.rect_filled(bar, 0.0, link_color);
            }
        }
    }
    let mut clicked_dot: Option<usize> = None;
    let mut clicked_link: Option<usize> = None;
    if response.clicked() {
        if let Some(p) = response.interact_pointer_pos() {
            clicked_dot = dot_centers
                .iter()
                .position(|c| (*c - p).length() <= radius + 1.0);
            if clicked_dot.is_none() {
                // Hit-test gap zones. Each zone spans its layout
                // x-range and the full dot-height row vertically.
                clicked_link = layout.gap_zones.iter().position(|zone| {
                    let zone_rect = egui::Rect::from_min_max(
                        egui::pos2(rect.left() + zone.x_start, cy - radius),
                        egui::pos2(rect.left() + zone.x_end, cy + radius),
                    );
                    zone_rect.contains(p)
                });
            }
        }
    }
    SocketsResponse {
        response,
        clicked_dot,
        clicked_link,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pob_data::parse_socket_string;

    fn cfg() -> SocketLayoutConfig {
        // Round numbers for readable expectations.
        SocketLayoutConfig {
            dot_diameter: 10.0,
            link_gap: 4.0,
            group_gap: 10.0,
            link_thickness: 2.0,
        }
    }

    #[test]
    fn empty_input_lays_out_nothing() {
        let layout = socket_render_layout(&[], cfg());
        assert!(layout.items.is_empty());
        assert_eq!(layout.width, 0.0);
    }

    #[test]
    fn single_socket_is_one_dot_no_links() {
        let groups = parse_socket_string("R");
        let layout = socket_render_layout(&groups, cfg());
        assert_eq!(layout.items.len(), 1);
        match layout.items[0] {
            RenderItem::Dot { x_center, color } => {
                assert_eq!(x_center, 5.0); // radius
                assert_eq!(color, SocketColor::Red);
            }
            other => panic!("expected dot, got {other:?}"),
        }
        assert_eq!(layout.width, 10.0);
    }

    #[test]
    fn three_link_emits_alternating_dots_and_links() {
        let groups = parse_socket_string("R-G-B");
        let layout = socket_render_layout(&groups, cfg());
        // dot, link, dot, link, dot
        assert_eq!(layout.items.len(), 5);
        assert!(matches!(layout.items[0], RenderItem::Dot { .. }));
        assert!(matches!(layout.items[1], RenderItem::Link { .. }));
        assert!(matches!(layout.items[2], RenderItem::Dot { .. }));
        assert!(matches!(layout.items[3], RenderItem::Link { .. }));
        assert!(matches!(layout.items[4], RenderItem::Dot { .. }));
        // 3 dots × 10 + 2 link gaps × 4 = 38
        assert_eq!(layout.width, 38.0);
    }

    #[test]
    fn two_groups_have_no_link_between_them() {
        let groups = parse_socket_string("R G");
        let layout = socket_render_layout(&groups, cfg());
        // Two dots, no link in between — the group gap is pure
        // whitespace, not a Link element.
        assert_eq!(layout.items.len(), 2);
        let centers: Vec<f32> = layout
            .items
            .iter()
            .filter_map(|i| match i {
                RenderItem::Dot { x_center, .. } => Some(*x_center),
                _ => None,
            })
            .collect();
        assert_eq!(centers, vec![5.0, 25.0]); // 5, then 5+10+10 (group gap span)
        assert_eq!(layout.width, 30.0);
    }

    #[test]
    fn mixed_3l_plus_2l_layout() {
        let groups = parse_socket_string("R-G-B G-W");
        let layout = socket_render_layout(&groups, cfg());
        // 5 + 1 (group gap) + 3 = group A produces 5 items; group B produces 3 items
        let dots: Vec<SocketColor> = layout
            .items
            .iter()
            .filter_map(|i| match i {
                RenderItem::Dot { color, .. } => Some(*color),
                _ => None,
            })
            .collect();
        assert_eq!(
            dots,
            vec![
                SocketColor::Red,
                SocketColor::Green,
                SocketColor::Blue,
                SocketColor::Green,
                SocketColor::White,
            ]
        );
        // 4 link bars total (2 + 1)? No — 2 in first group, 1 in second = 3
        let links = layout
            .items
            .iter()
            .filter(|i| matches!(i, RenderItem::Link { .. }))
            .count();
        assert_eq!(links, 3);
        // width: 38 (first group) + 10 (group gap) + 24 (second: 2*10 + 4 link) = 72
        assert_eq!(layout.width, 72.0);
    }

    #[test]
    fn empty_groups_are_skipped() {
        // Synthesised input — `parse_socket_string` doesn't produce
        // empty groups, but defensive code handles them anyway.
        let groups = vec![
            SocketGroup { colors: vec![] },
            SocketGroup {
                colors: vec![SocketColor::Red],
            },
            SocketGroup { colors: vec![] },
        ];
        let layout = socket_render_layout(&groups, cfg());
        assert_eq!(layout.items.len(), 1);
        assert_eq!(layout.width, 10.0);
    }

    #[test]
    fn apply_socket_cycle_advances_color_at_index() {
        // First dot in a 3-link cycles R → G; the rest of the layout
        // (the `-` link bars) survives intact.
        assert_eq!(apply_socket_cycle_at("R-G-B", 0), "G-G-B");
        // Mid-group socket cycles independently.
        assert_eq!(apply_socket_cycle_at("R-G-B", 1), "R-B-B");
        // Last socket in a 3-link.
        assert_eq!(apply_socket_cycle_at("R-G-B", 2), "R-G-W");
    }

    #[test]
    fn apply_socket_cycle_wraps_w_back_to_r() {
        assert_eq!(apply_socket_cycle_at("W", 0), "R");
    }

    #[test]
    fn apply_socket_cycle_indexes_across_multiple_groups() {
        // The dot index counts across every group, ignoring the
        // group separator (` `) and link bars (`-`). So `2` here
        // points at the third dot (the first dot of the second
        // group) — `B`, which cycles to `W`.
        assert_eq!(apply_socket_cycle_at("R-G B-W", 2), "R-G W-W");
    }

    #[test]
    fn apply_socket_cycle_keeps_abyss_fixed() {
        // Abyss sockets are fixed by the base — `cycle_next` is a
        // no-op on abyss, so the helper passes through unchanged.
        // (`A-A`-style abyss layouts come from belts with two abyss
        // sockets — clicking one still has to be valid input even
        // if the resulting string is the same.)
        assert_eq!(apply_socket_cycle_at("A", 0), "A");
        assert_eq!(apply_socket_cycle_at("A A", 1), "A A");
    }

    #[test]
    fn apply_socket_cycle_out_of_range_index_is_noop() {
        // Defensive against a stale click — if the user typed a
        // shorter socket string between the click and the frame
        // running this helper, the index could fall past the end.
        // The helper should just return the input unchanged rather
        // than panic / wrap into the wrong socket.
        assert_eq!(apply_socket_cycle_at("R-G", 5), "R-G");
        assert_eq!(apply_socket_cycle_at("", 0), "");
    }

    #[test]
    fn apply_socket_cycle_preserves_unusual_separators_verbatim() {
        // The parser tolerates `,` and other separators (see
        // `parse_socket_string`); the cycle helper should preserve
        // them too so a round-trip through "parse → render →
        // cycle → emit" doesn't normalise the user's formatting.
        assert_eq!(apply_socket_cycle_at("R,G,B", 1), "R,B,B");
    }

    #[test]
    fn apply_socket_link_toggle_breaks_link_in_group() {
        // First gap of `R-G-B` is the link between R and G. Toggling
        // breaks it — the group splits in two so the resulting string
        // has a space instead of `-` at that position.
        assert_eq!(apply_socket_link_toggle_at("R-G-B", 0), "R G-B");
        // Second gap is the link between G and B — toggling that one
        // only.
        assert_eq!(apply_socket_link_toggle_at("R-G-B", 1), "R-G B");
    }

    #[test]
    fn apply_socket_link_toggle_creates_link_between_groups() {
        // `R G` is two unlinked sockets. Toggling the gap between them
        // re-links them, producing `R-G`.
        assert_eq!(apply_socket_link_toggle_at("R G", 0), "R-G");
    }

    #[test]
    fn apply_socket_link_toggle_index_counts_gaps_left_to_right() {
        // `R-G B-W` has three gaps: link, group, link. Index 1 is the
        // inter-group gap; toggling re-links all four sockets.
        assert_eq!(apply_socket_link_toggle_at("R-G B-W", 1), "R-G-B-W");
    }

    #[test]
    fn apply_socket_link_toggle_out_of_range_is_noop() {
        // Defensive against stale clicks or empty inputs.
        assert_eq!(apply_socket_link_toggle_at("R-G", 5), "R-G");
        assert_eq!(apply_socket_link_toggle_at("R", 0), "R");
        assert_eq!(apply_socket_link_toggle_at("", 0), "");
    }

    #[test]
    fn apply_socket_link_toggle_refuses_to_link_abyss() {
        // Abyss sockets can't be part of a link group — they're jewel
        // sockets. Toggling a gap adjacent to an abyss socket is a
        // no-op so a stale click can't produce an invalid socket
        // string.
        assert_eq!(apply_socket_link_toggle_at("R A", 0), "R A");
        assert_eq!(apply_socket_link_toggle_at("A R", 0), "A R");
        assert_eq!(apply_socket_link_toggle_at("A A", 0), "A A");
    }

    #[test]
    fn layout_emits_gap_zones_for_links_and_group_gaps() {
        // The layout reports a `GapZone` for every adjacent socket
        // pair, distinguishing currently-linked gaps (with a Link
        // element in `items`) from inter-group gaps (no Link). The
        // renderer hit-tests against these to support click-to-toggle.
        let groups = parse_socket_string("R-G B");
        let layout = socket_render_layout(&groups, cfg());
        assert_eq!(layout.gap_zones.len(), 2);
        assert!(layout.gap_zones[0].linked, "first gap is a link");
        assert!(!layout.gap_zones[1].linked, "second gap is inter-group");
    }

    #[test]
    fn layout_emits_no_gap_zones_for_single_socket() {
        let groups = parse_socket_string("R");
        let layout = socket_render_layout(&groups, cfg());
        assert!(layout.gap_zones.is_empty());
    }

    #[test]
    fn abyss_socket_uses_distinct_fill() {
        // Quick sanity that abyss isn't colliding with any RGBW colour
        // in the palette — they should all be distinct so the user can
        // read the socket type at a glance.
        let palette = [
            socket_fill(SocketColor::Red),
            socket_fill(SocketColor::Green),
            socket_fill(SocketColor::Blue),
            socket_fill(SocketColor::White),
            socket_fill(SocketColor::Abyss),
        ];
        for i in 0..palette.len() {
            for j in (i + 1)..palette.len() {
                assert_ne!(palette[i], palette[j], "colours {i}/{j} collide");
            }
        }
    }
}
