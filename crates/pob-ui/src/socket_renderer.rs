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
//! Click-to-cycle (`SocketColor::cycle_next`) is intentionally NOT
//! wired in this slice — the Items tab today reads `item.sockets`
//! straight off the `Item` struct, so a click would need to either
//! mutate the parsed item back to the string or refactor storage.
//! Both belong in a follow-up; this slice is render-only.

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

/// Result of laying out a list of socket groups in a horizontal row.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SocketLayout {
    pub items: Vec<RenderItem>,
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
    let mut x = 0.0_f32;
    let radius = cfg.dot_diameter * 0.5;
    let mut first_group = true;
    for group in groups {
        if group.is_empty() {
            continue;
        }
        if !first_group {
            x += cfg.group_gap;
        }
        first_group = false;
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
            }
            x += radius;
            items.push(RenderItem::Dot {
                x_center: x,
                color: *color,
            });
            x += radius;
        }
    }
    SocketLayout { items, width: x }
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

/// Paint the supplied socket groups inline into `ui`. Allocates a
/// horizontal slot of exactly the layout width × dot height and draws
/// the dots + link bars into it. Returns the egui `Response` for
/// hover/click wiring later.
pub fn draw_sockets(
    ui: &mut egui::Ui,
    groups: &[SocketGroup],
    cfg: SocketLayoutConfig,
) -> egui::Response {
    let layout = socket_render_layout(groups, cfg);
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(layout.width.max(1.0), cfg.dot_diameter),
        egui::Sense::hover(),
    );
    let painter = ui.painter_at(rect);
    let radius = cfg.dot_diameter * 0.5;
    let cy = rect.center().y;
    let stroke_color = ui.visuals().widgets.noninteractive.bg_stroke.color;
    let link_color = ui.visuals().widgets.noninteractive.fg_stroke.color;
    for item in &layout.items {
        match item {
            RenderItem::Dot { x_center, color } => {
                let center = egui::pos2(rect.left() + *x_center, cy);
                painter.circle_filled(center, radius, socket_fill(*color));
                // Thin outline so light dots (white) read on a light bg.
                painter.circle_stroke(center, radius, egui::Stroke::new(1.0, stroke_color));
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
    response
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
