//! Issue #211 — reusable sortable / filterable list-control widget.
//!
//! Reference PoB ships a single `Classes/ListControl.lua` base widget that
//! the items / skills / specs lists all derive from. MK2's lists were each
//! growing their own one-off filter + sort logic; this module collapses
//! the shared pieces into a small reusable surface so the items, skills,
//! catalogs etc. can all opt in incrementally.
//!
//! Design goals:
//! - **Pure helpers first.** The interesting bits — applying a column
//!   filter, computing the sorted index order — are plain functions over
//!   a generic row type. That keeps the unit tests tiny and means the
//!   logic is reusable from anywhere (e.g. the gem catalog, the build
//!   browser) without dragging an egui context in.
//! - **State is small + serialisable-ish.** `SortState<C>` is just a
//!   column id and a direction, so callers can persist it across tab
//!   switches by sticking it in their tab state struct. The issue's
//!   acceptance criterion that "sort state persists across tab switches
//!   in the session" falls out for free — the tab state already lives in
//!   `LoadedApp` for the session.
//! - **UI helper is opt-in.** Tabs that already have a custom header row
//!   can call only the sort/filter helpers; tabs that want clickable
//!   column headers wired up for them call [`column_header`].
//!
//! Tabs already adopting the helpers in this slice:
//! - `items_tab` browse panel (sortable name + class columns, persistent
//!   sort state). The wider items list (slot grid) is intrinsically
//!   single-row-per-slot so doesn't benefit from sorting.
//!
//! Follow-ups tracked in the PR description:
//! - Gem catalog in `skills_tab` (sort by name / color / level).
//! - Saved-spec list once we ship a passive-spec switcher (no current UI).
//! - Right-click context menus + multi-select are scoped out of this slice
//!   to keep the diff focused on the sort/filter primitives.

use std::cmp::Ordering;

use eframe::egui;

/// Direction of a column sort. `Asc` is the default the first time a
/// user clicks an unsorted column; clicking the active column flips to
/// `Desc`; clicking a third time clears the sort (returns to dataset
/// order).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDirection {
    Asc,
    Desc,
}

impl SortDirection {
    /// Glyph rendered next to a sorted column's header label. Matches the
    /// triangle convention upstream PoB uses in its list controls.
    pub fn glyph(self) -> &'static str {
        match self {
            Self::Asc => "▲",
            Self::Desc => "▼",
        }
    }

    /// Reverse this direction — used when applying a sort so callers can
    /// pre-compute an `Asc` comparator and let the helper flip it. Kept
    /// public for callers that drive a custom sort path (the in-tree
    /// `sorted_indices` helper inverts via `Ordering::reverse` directly).
    #[allow(dead_code)]
    pub fn reverse(self) -> Self {
        match self {
            Self::Asc => Self::Desc,
            Self::Desc => Self::Asc,
        }
    }
}

/// Persistent sort state for a single list. `C` is whatever enum / id
/// the caller uses to discriminate columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SortState<C> {
    pub column: C,
    pub direction: SortDirection,
}

impl<C> SortState<C> {
    pub fn new(column: C, direction: SortDirection) -> Self {
        Self { column, direction }
    }
}

/// Click-cycle helper for column headers. Encodes the standard three-state
/// cycle PoB uses: clicking an unsorted column sorts ascending; clicking
/// the active sorted column flips direction; clicking again clears the
/// sort (returns `None`). Pulled out as a pure function so the cycle is
/// trivially unit-testable.
#[must_use]
pub fn cycle_sort<C: PartialEq + Copy>(
    current: Option<SortState<C>>,
    clicked: C,
) -> Option<SortState<C>> {
    match current {
        // First click on this column — start ascending.
        None => Some(SortState::new(clicked, SortDirection::Asc)),
        Some(state) if state.column != clicked => {
            // Clicking a different column starts a fresh ascending sort.
            Some(SortState::new(clicked, SortDirection::Asc))
        }
        Some(state) => match state.direction {
            SortDirection::Asc => Some(SortState::new(clicked, SortDirection::Desc)),
            // Third click: clear back to dataset order.
            SortDirection::Desc => None,
        },
    }
}

/// Pure helper: produce a permutation of `rows` indices ordered by the
/// supplied comparator and direction. Returns `None` (i.e. natural order)
/// when `state` is `None` — callers iterate `0..rows.len()` in that case.
///
/// We return indices instead of cloning rows so callers can keep using
/// borrowed references / IndexMap iteration order downstream.
#[must_use]
pub fn sorted_indices<R, C, F>(
    rows: &[R],
    state: Option<SortState<C>>,
    mut compare_asc: F,
) -> Vec<usize>
where
    F: FnMut(&R, &R, C) -> Ordering,
    C: Copy,
{
    let mut idx: Vec<usize> = (0..rows.len()).collect();
    let Some(state) = state else { return idx };
    idx.sort_by(|a, b| {
        let ord = compare_asc(&rows[*a], &rows[*b], state.column);
        match state.direction {
            SortDirection::Asc => ord,
            SortDirection::Desc => ord.reverse(),
        }
    });
    idx
}

/// Case-insensitive substring filter over an iterator of haystack
/// strings. The empty-query case is hot — the gem catalog re-runs this
/// on every frame — so we short-circuit before lower-casing.
///
/// This is the building block all per-column text filters share. Callers
/// supply whatever fields they want searchable as the haystack iterator.
#[must_use]
pub fn text_filter_matches<'a, I>(query: &str, haystacks: I) -> bool
where
    I: IntoIterator<Item = &'a str>,
{
    let q = query.trim();
    if q.is_empty() {
        return true;
    }
    let q_lower = q.to_ascii_lowercase();
    for h in haystacks {
        if h.to_ascii_lowercase().contains(&q_lower) {
            return true;
        }
    }
    false
}

/// Render a clickable column header. Returns `true` if the user clicked
/// — caller is expected to feed that into [`cycle_sort`] to advance the
/// sort state. The active column gets a direction glyph appended; the
/// inactive columns get a faint dot so it's still obvious they're
/// clickable.
///
/// We don't take ownership of the sort state itself — callers thread
/// their own `SortState` through because the column id type `C` lives
/// in their module.
pub fn column_header<C: PartialEq + Copy>(
    ui: &mut egui::Ui,
    label: &str,
    column: C,
    sort: Option<SortState<C>>,
) -> bool {
    let glyph = match sort {
        Some(s) if s.column == column => s.direction.glyph(),
        _ => "·",
    };
    let text = format!("{label} {glyph}");
    ui.add(egui::Button::new(egui::RichText::new(text).strong()).frame(false))
        .on_hover_text(
            "Click to sort. Click again to reverse. Click a third time to clear the sort.",
        )
        .clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tiny column id used by the unit tests below. Mirrors how a real
    /// caller would wire up an enum (items: Name / Type / Slot, etc.).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Col {
        Name,
        Score,
    }

    #[derive(Debug, Clone)]
    struct Row {
        name: &'static str,
        score: i32,
    }

    fn rows() -> Vec<Row> {
        vec![
            Row {
                name: "Gamma",
                score: 5,
            },
            Row {
                name: "alpha",
                score: 12,
            },
            Row {
                name: "Beta",
                score: 7,
            },
        ]
    }

    fn cmp(a: &Row, b: &Row, col: Col) -> Ordering {
        match col {
            Col::Name => a
                .name
                .to_ascii_lowercase()
                .cmp(&b.name.to_ascii_lowercase()),
            Col::Score => a.score.cmp(&b.score),
        }
    }

    #[test]
    fn cycle_sort_walks_three_states() {
        let s1 = cycle_sort::<Col>(None, Col::Name);
        assert_eq!(s1, Some(SortState::new(Col::Name, SortDirection::Asc)));
        let s2 = cycle_sort(s1, Col::Name);
        assert_eq!(s2, Some(SortState::new(Col::Name, SortDirection::Desc)));
        let s3 = cycle_sort(s2, Col::Name);
        assert_eq!(s3, None);
    }

    #[test]
    fn cycle_sort_switching_column_resets_to_asc() {
        let active = Some(SortState::new(Col::Name, SortDirection::Desc));
        let next = cycle_sort(active, Col::Score);
        assert_eq!(next, Some(SortState::new(Col::Score, SortDirection::Asc)));
    }

    #[test]
    fn sorted_indices_natural_order_when_no_state() {
        let r = rows();
        let order = sorted_indices(&r, None::<SortState<Col>>, cmp);
        assert_eq!(order, vec![0, 1, 2]);
    }

    #[test]
    fn sorted_indices_orders_by_name_case_insensitive() {
        let r = rows();
        let order = sorted_indices(&r, Some(SortState::new(Col::Name, SortDirection::Asc)), cmp);
        // alpha < Beta < Gamma
        assert_eq!(order, vec![1, 2, 0]);

        let order = sorted_indices(
            &r,
            Some(SortState::new(Col::Name, SortDirection::Desc)),
            cmp,
        );
        assert_eq!(order, vec![0, 2, 1]);
    }

    #[test]
    fn sorted_indices_orders_by_score() {
        let r = rows();
        let order = sorted_indices(
            &r,
            Some(SortState::new(Col::Score, SortDirection::Asc)),
            cmp,
        );
        // 5 < 7 < 12
        assert_eq!(order, vec![0, 2, 1]);
    }

    #[test]
    fn text_filter_short_circuits_on_empty_query() {
        // No haystacks are inspected when the query is empty — the
        // empty iterator below would otherwise trivially return false.
        assert!(text_filter_matches("", std::iter::empty::<&str>()));
        assert!(text_filter_matches("   ", std::iter::empty::<&str>()));
    }

    #[test]
    fn text_filter_is_case_insensitive_substring() {
        let row = ["One Handed Sword", "Weapon"];
        assert!(text_filter_matches("HAND", row.iter().copied()));
        assert!(text_filter_matches("weapon", row.iter().copied()));
        assert!(!text_filter_matches("bow", row.iter().copied()));
    }

    #[test]
    fn text_filter_trims_query() {
        let row = ["Onyx Amulet"];
        assert!(text_filter_matches("  amulet  ", row.iter().copied()));
    }

    #[test]
    fn sort_direction_reverse_and_glyph() {
        assert_eq!(SortDirection::Asc.reverse(), SortDirection::Desc);
        assert_eq!(SortDirection::Desc.reverse(), SortDirection::Asc);
        assert_eq!(SortDirection::Asc.glyph(), "▲");
        assert_eq!(SortDirection::Desc.glyph(), "▼");
    }
}
