//! Shared switcher-dropdown helpers for "set" tabs (item / skill / config).
//!
//! Issue #222 covers a per-tab `egui::ComboBox` that lets the user flip
//! between saved sets without scrolling the inline chip row. PR #385 shipped
//! the **item-set** switcher with two pure helpers — `format_set_dropdown_label`
//! and `shift_active_idx_after_delete`. Both helpers are entirely UI-agnostic
//! (no `egui`, no `Character`), so they belong in a shared module that the
//! sibling switchers can re-use as their model support lands.
//!
//! ## Why this module exists today (and not the full skill / config slices)
//!
//! PoB Lua exposes three parallel switchers:
//! * [`Classes/ItemsTab.lua` `controls.itemSet`] → MK2 `Character::item_sets`
//!   (live since the issue #212 / #376 / #385 chain).
//! * [`Classes/SkillsTab.lua:95-106` `controls.skillSet`] → no MK2 equivalent
//!   yet. `Character::skill_groups` is the gem-link list *inside one set*, not
//!   a vector of named saved sets.
//! * [`Classes/ConfigTab.lua:41-53` `controls.configSet`] → also missing in
//!   MK2; `Character` has scalar config fields, not a `Vec<NamedConfigSet>`.
//!
//! Until the engine grows `skill_sets` / `config_sets` (its own slice — needs
//! XML round-trip, persistence, and undo coverage), the switcher dropdowns
//! cannot be wired. By extracting the two helpers here we lock down the
//! presentation contract (label format, delete-shift rule) so the future
//! skill / config slices can wire `egui::ComboBox::from_label(...)` against
//! the same well-tested helpers and stay visually consistent with the item
//! switcher.
//!
//! Both helpers were originally introduced in `items_tab.rs`; `items_tab`
//! now re-exports them from here so existing call sites (and the eight
//! original unit tests over there) keep working unchanged.

/// Issue #222: format the dropdown label shown for each saved set in a switcher
/// [`egui::ComboBox`].
///
/// The label is tagged with the 1-based index so users can map the dropdown
/// to the inline buttons / manage popup, and with a small marker when the
/// entry is the currently active set so the *closed* combo communicates
/// "this is the live set" without an extra label nearby.
///
/// Inactive rows get two leading spaces so names line up vertically with
/// active rows (the marker glyph is one column, plus the trailing space).
///
/// Empty / whitespace-only names render as `(unnamed)` so the entry is still
/// pickable; an empty label inside the combo box renders as a void.
///
/// Pure / no-`egui` so it runs in the unit-test loop. The `is_active` input
/// is derived in the UI from each tab's "remembered active idx" state field
/// (e.g. [`crate::items_tab::ItemsTabState::active_item_set_idx`]).
pub fn format_set_dropdown_label(name: &str, idx: usize, is_active: bool) -> String {
    let trimmed = name.trim();
    let display = if trimmed.is_empty() {
        "(unnamed)"
    } else {
        trimmed
    };
    let one_based = idx + 1;
    if is_active {
        format!("● {one_based}. {display}")
    } else {
        format!("  {one_based}. {display}")
    }
}

/// Issue #222: shift the switcher's remembered active index after a delete.
///
/// `deleted_idx` is the position the user just removed from the saved-set
/// vector. The active marker should:
/// * **clear** when the active entry itself was removed — no successor is
///   implied (the entry below shifts up but isn't necessarily what the user
///   wants),
/// * **shift down by one** when an entry *before* the active one was removed
///   so the marker still points at the same set,
/// * **stay put** for deletes after the active entry.
///
/// Pulled into a pure helper so the rule is documented and unit-testable in
/// isolation — the same shape applies to item / skill / config sets.
pub fn shift_active_idx_after_delete(active: Option<usize>, deleted_idx: usize) -> Option<usize> {
    let active = active?;
    match active.cmp(&deleted_idx) {
        std::cmp::Ordering::Equal => None,
        std::cmp::Ordering::Greater => Some(active - 1),
        std::cmp::Ordering::Less => Some(active),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── format_set_dropdown_label ───────────────────────────────────────

    #[test]
    fn label_marks_active_entry_with_filled_glyph() {
        // Active rows get the filled glyph so the closed combo communicates
        // "this is the live set" without an extra label nearby.
        let label = format_set_dropdown_label("Tank", 0, true);
        assert!(label.starts_with('●'), "expected active glyph, got {label}");
        assert!(
            label.contains("1. Tank"),
            "expected 1-based index, got {label}"
        );
    }

    #[test]
    fn label_pads_inactive_entry_for_alignment() {
        // Inactive rows align with active rows by leading whitespace so
        // names line up vertically inside the dropdown.
        let label = format_set_dropdown_label("DPS", 2, false);
        assert!(
            !label.starts_with('●'),
            "no active glyph for inactive: {label}"
        );
        assert!(
            label.contains("3. DPS"),
            "expected 1-based index, got {label}"
        );
        assert!(
            label.starts_with("  "),
            "expected leading padding, got {label:?}"
        );
    }

    #[test]
    fn label_substitutes_blank_name_with_placeholder() {
        // Empty / whitespace-only names would render as a void in the
        // combo; substitute a placeholder so the entry is still pickable.
        let label = format_set_dropdown_label("   ", 4, false);
        assert!(
            label.contains("(unnamed)"),
            "expected placeholder, got {label}"
        );
        assert!(label.contains("5."), "expected 1-based index, got {label}");
    }

    #[test]
    fn label_uses_placeholder_for_truly_empty_name() {
        // Distinct case from the whitespace one: empty string takes the
        // same branch but exercises the trim → empty → placeholder path
        // without relying on `trim` collapsing spaces.
        let label = format_set_dropdown_label("", 0, true);
        assert!(label.contains("(unnamed)"), "expected placeholder: {label}");
        assert!(label.contains("1."), "expected 1-based index: {label}");
    }

    #[test]
    fn label_trims_surrounding_whitespace_around_real_name() {
        // Trim ensures the dropdown displays the canonical name even if
        // the user's saved name accidentally has leading / trailing spaces
        // (the engine's rename helpers don't currently force a trim).
        let label = format_set_dropdown_label("  Boss  ", 0, false);
        assert!(
            label.ends_with("Boss"),
            "expected trimmed name at tail, got {label:?}"
        );
        assert!(
            !label.contains("  Boss"),
            "untrimmed leading whitespace leaked into label: {label:?}"
        );
    }

    #[test]
    fn label_handles_unicode_and_long_names_verbatim() {
        // Names are user-supplied; the helper should not truncate or
        // sanitise them. egui's combo handles its own text layout.
        let long = "極めて長い名前 — gauntlet of the eternal night";
        let label = format_set_dropdown_label(long, 9, true);
        assert!(label.contains(long), "long unicode name lost: {label}");
        assert!(label.contains("10."), "expected 1-based index, got {label}");
    }

    #[test]
    fn label_handles_high_idx_without_overflow() {
        // 1-based conversion adds one; `usize::MAX` would overflow. The
        // helper relies on the caller passing in-range indices (a real
        // saved-set vector caps out at the engine's persistence limit),
        // but a plausibly-large index should still render.
        let label = format_set_dropdown_label("set", 999, false);
        assert!(label.contains("1000. set"), "got {label}");
    }

    // ─── shift_active_idx_after_delete ───────────────────────────────────

    #[test]
    fn shift_clears_when_self_deleted() {
        // Deleting the entry the switcher points at clears the marker —
        // there's no obvious "next" to advance to.
        assert_eq!(shift_active_idx_after_delete(Some(2), 2), None);
        assert_eq!(shift_active_idx_after_delete(Some(0), 0), None);
    }

    #[test]
    fn shift_decrements_when_earlier_deleted() {
        // Deleting an entry *before* the active one keeps the marker on
        // the same set, just at a lower index.
        assert_eq!(shift_active_idx_after_delete(Some(3), 1), Some(2));
        assert_eq!(shift_active_idx_after_delete(Some(1), 0), Some(0));
        assert_eq!(shift_active_idx_after_delete(Some(10), 0), Some(9));
    }

    #[test]
    fn shift_unaffected_by_later_deletes() {
        // Deleting an entry past the active one doesn't move the marker.
        assert_eq!(shift_active_idx_after_delete(Some(0), 1), Some(0));
        assert_eq!(shift_active_idx_after_delete(Some(2), 5), Some(2));
    }

    #[test]
    fn shift_passes_through_no_active_marker() {
        // No active marker stays no active marker.
        assert_eq!(shift_active_idx_after_delete(None, 0), None);
        assert_eq!(shift_active_idx_after_delete(None, 9), None);
    }
}
