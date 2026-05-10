//! Reusable popup / modal-dialog / tooltip-host infrastructure.
//!
//! Issue [#224](https://github.com/jonatanferm/pathofbuildingmk2/issues/224) — a
//! shared foundation for the many UI features that need to "open a small
//! window over the rest of the app and wait for the user to interact":
//! mastery picker (#210), notable DB (#215), timeless-jewel socket UI
//! (#216), gem picker (#208), item DB (#209), set-manager popups (#222),
//! enchant / anoint pickers, reset / version-converter dialogs (#220),
//! rich tree / item / mod tooltips (#203), …
//!
//! ## Design
//!
//! Upstream PoB exposes a *popup stack* (`Main.lua:62 main.popups`,
//! backed by `Classes/PopupDialog.lua`) and a *TooltipHost* layer
//! (`Classes/Tooltip.lua` + `Classes/TooltipHost.lua`). The popup stack
//! is LIFO — newer dialogs visually and input-wise occlude older ones —
//! and the top dialog steals the keyboard. The tooltip host accepts
//! rich content: multi-line bodies with inline `^N` / `^xRRGGBB` colour
//! escapes, multiple sections separated by rules, and (in some cases)
//! embedded controls.
//!
//! This module mirrors that with a *minimal* request-popup API and a
//! single host that draws the active dialog on top of every other UI:
//!
//! - [`PopupHost`] owns a LIFO stack of [`PopupRequest`]s, each tagged
//!   with a stable [`PopupId`]. A tab calls [`PopupHost::open`] with a
//!   request, and on subsequent frames calls [`PopupHost::take_top`] to
//!   pull the active request, render its body via `egui::Window`, and
//!   re-push it (or close it) based on the user's interaction.
//! - The host itself is *content-agnostic*: it doesn't know how to draw
//!   a mastery picker or an item DB browser. Those tabs keep their own
//!   per-popup state structs (e.g. `MasteryPickerState`,
//!   `TattooPickerState`); they just route their visibility through
//!   `PopupHost` so dialog stacking, dismissal, and keyboard focus work
//!   uniformly. See `tattoo_picker.rs` / `mastery_picker.rs` for the
//!   pattern.
//! - [`TooltipBody`] is a small data type for *rich* tooltip content:
//!   one or more sections, each with a list of lines that may contain
//!   `^N` / `^xRRGGBB` colour escapes (parsed by
//!   [`crate::color_codes::to_layout_job`]). [`show_rich_tooltip`]
//!   renders one inside any `egui::Response::on_hover_ui_at_pointer`
//!   closure, so callers can attach formatted breakdown tooltips to any
//!   widget without re-implementing the layout.
//!
//! ## What this module does *not* try to be
//!
//! - It is not a layout engine — popups still call `egui::Window` /
//!   `egui::Area` directly; the host just enforces stacking order and
//!   centralised dismissal (Esc, click-outside on the topmost modal,
//!   programmatic close from a tab).
//! - It does not own or render the tab's domain state. The host stores
//!   only the *request* (id + title + sizing); the picker / dialog code
//!   stays in its own module.
//! - It is not async. Popups are synchronous immediate-mode dialogs.
//!
//! ## Adoption pattern
//!
//! ```ignore
//! // 1. Each tab declares its own popup IDs:
//! const TATTOO_PICKER_POPUP: PopupId = PopupId::from_static("tattoo-picker");
//!
//! // 2. When the tab wants to open the dialog:
//! popup_host.open(PopupRequest::modal(TATTOO_PICKER_POPUP, "Apply tattoo"));
//!
//! // 3. Each frame the central UI loop drains the top request, hands it
//! //    to the matching tab module, and the tab pushes the request back
//! //    via `keep_open` (or drops it to close):
//! if let Some(req) = popup_host.take_top() {
//!     match req.id() {
//!         TATTOO_PICKER_POPUP => tattoo_picker::ui(ctx, &mut state, …),
//!         _ => { popup_host.open(req); } // unknown — re-push to keep stack stable.
//!     }
//! }
//! ```
//!
//! Today only the foundation lives here; the tabs that already own their
//! own ad-hoc dialogs (mastery picker, tattoo picker, tree-reset modal)
//! continue to work unchanged. They will migrate onto the host one tab
//! at a time as the dependent issues are picked up.

use egui::{Color32, FontId, Response};

use crate::color_codes;

/// Stable identifier for a popup. Tabs declare their popups as `const`s
/// so the central dispatcher can match on them. Backed by a static
/// string so `PopupId` remains `Copy` and cheap to compare.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PopupId(&'static str);

impl PopupId {
    /// Construct a `PopupId` from a static string slice. Conventionally
    /// kebab-case, scoped by tab — `"tree.tattoo-picker"`,
    /// `"items.gem-picker"`, etc. — to avoid collisions across tabs.
    pub const fn from_static(s: &'static str) -> Self {
        Self(s)
    }

    /// Borrow the underlying identifier.
    pub fn as_str(self) -> &'static str {
        self.0
    }
}

impl std::fmt::Display for PopupId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

/// How a popup interacts with the rest of the UI.
///
/// Mirrors PoB's split between modal `PopupDialog`s (which steal input
/// until dismissed) and lightweight, click-away "context" popups (e.g.
/// the build-list row's right-click menu). Today the host treats both
/// identically at the rendering layer — the distinction lives here so
/// future passes can wire input gating once tabs have migrated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PopupKind {
    /// Blocks input to the rest of the UI until dismissed. Reset
    /// confirmation, picker dialogs, file-conflict prompts.
    Modal,
    /// Floats over the UI but does not block. Right-click menus,
    /// transient hover popovers.
    Floating,
}

/// A single entry on the popup stack. Carries the bookkeeping the host
/// needs (stable id, title, sizing) but not the body — rendering the
/// body is the calling tab's job (see module docs).
#[derive(Debug, Clone)]
pub struct PopupRequest {
    id: PopupId,
    title: String,
    kind: PopupKind,
    /// Default width, in egui points. `None` lets egui size to content.
    default_width: Option<f32>,
    /// Default height, in egui points. `None` lets egui size to content.
    default_height: Option<f32>,
    /// Anchor the popup window to screen-center? PoB modals are
    /// always centered; floating popups usually open near the cursor.
    centered: bool,
}

impl PopupRequest {
    /// Build a centered modal dialog with default sizing.
    pub fn modal(id: PopupId, title: impl Into<String>) -> Self {
        Self {
            id,
            title: title.into(),
            kind: PopupKind::Modal,
            default_width: None,
            default_height: None,
            centered: true,
        }
    }

    /// Build a floating (non-blocking) popup, positioned by egui.
    pub fn floating(id: PopupId, title: impl Into<String>) -> Self {
        Self {
            id,
            title: title.into(),
            kind: PopupKind::Floating,
            default_width: None,
            default_height: None,
            centered: false,
        }
    }

    /// Set a default width (in egui points).
    #[must_use]
    pub fn with_default_width(mut self, w: f32) -> Self {
        self.default_width = Some(w);
        self
    }

    /// Set a default height (in egui points).
    #[must_use]
    pub fn with_default_height(mut self, h: f32) -> Self {
        self.default_height = Some(h);
        self
    }

    /// Override the centering behaviour. Defaults to `true` for modals,
    /// `false` for floating popups.
    #[must_use]
    pub fn with_centered(mut self, centered: bool) -> Self {
        self.centered = centered;
        self
    }

    /// Identifier this request was opened with.
    pub fn id(&self) -> PopupId {
        self.id
    }

    /// Window title shown to the user.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Whether this is a modal (blocking) or floating popup.
    pub fn kind(&self) -> PopupKind {
        self.kind
    }

    /// Convenience: open an `egui::Window` configured per this request
    /// and run `add_contents` inside it. Returns the egui inner-response
    /// option (None when the window was minimised). The caller is
    /// responsible for re-pushing the request onto the host if it wants
    /// to remain open across frames.
    pub fn show<R>(
        &self,
        ctx: &egui::Context,
        open: &mut bool,
        add_contents: impl FnOnce(&mut egui::Ui) -> R,
    ) -> Option<egui::InnerResponse<Option<R>>> {
        let mut window = egui::Window::new(self.title.as_str())
            .id(egui::Id::new(("popup-host", self.id.as_str())))
            .open(open)
            .collapsible(false)
            .resizable(matches!(self.kind, PopupKind::Modal));
        if self.centered {
            window = window.anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]);
        }
        if let Some(w) = self.default_width {
            window = window.default_width(w);
        }
        if let Some(h) = self.default_height {
            window = window.default_height(h);
        }
        window.show(ctx, add_contents)
    }
}

/// LIFO stack of active popups.
///
/// One `PopupHost` lives on the top-level app state. Tabs push requests
/// when they want to open a dialog, and the central UI loop pops the
/// top request each frame, rendering it on top of everything else.
///
/// The host itself is `Default`-constructible and contains no
/// egui-specific state — it stores only the requests. That keeps it
/// easy to unit-test in isolation (no `egui::Context`, no headless
/// renderer) while still capturing the LIFO + per-id semantics that the
/// upstream `Classes/PopupDialog.lua` relies on.
#[derive(Default, Debug, Clone)]
pub struct PopupHost {
    /// Stack ordered oldest-first. The *top* is the last element.
    stack: Vec<PopupRequest>,
}

impl PopupHost {
    /// Construct an empty host.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether anything is currently open.
    pub fn is_empty(&self) -> bool {
        self.stack.is_empty()
    }

    /// Number of popups on the stack.
    pub fn len(&self) -> usize {
        self.stack.len()
    }

    /// Push a popup onto the stack. If a popup with the same `id` is
    /// already present it is moved to the top instead of duplicated —
    /// PoB's behaviour for re-opening a dialog that was already showing
    /// is to bring it to the front, not stack two copies.
    pub fn open(&mut self, request: PopupRequest) {
        self.close_by_id(request.id);
        self.stack.push(request);
    }

    /// Peek at the topmost popup without removing it.
    pub fn top(&self) -> Option<&PopupRequest> {
        self.stack.last()
    }

    /// Whether `id` is the topmost popup.
    pub fn is_top(&self, id: PopupId) -> bool {
        self.stack.last().map(|r| r.id) == Some(id)
    }

    /// Whether `id` is anywhere on the stack.
    pub fn is_open(&self, id: PopupId) -> bool {
        self.stack.iter().any(|r| r.id == id)
    }

    /// Close the popup with the given id (no-op if not present).
    pub fn close_by_id(&mut self, id: PopupId) {
        self.stack.retain(|r| r.id != id);
    }

    /// Pop the topmost popup. Use with [`Self::open`] to "consume → maybe
    /// re-push" inside a single frame's render pass.
    pub fn take_top(&mut self) -> Option<PopupRequest> {
        self.stack.pop()
    }

    /// Close every open popup. Intended for global handlers ("escape
    /// when stack is non-empty closes everything", "switch builds clears
    /// any open dialog", …).
    pub fn close_all(&mut self) {
        self.stack.clear();
    }

    /// Read-only iterator over the stack, oldest-first.
    pub fn iter(&self) -> impl Iterator<Item = &PopupRequest> + '_ {
        self.stack.iter()
    }
}

/// Rich tooltip body. One or more sections, each a sequence of lines
/// that may contain `^N` / `^xRRGGBB` colour escapes
/// (see [`crate::color_codes`]).
///
/// Sections are separated by a horizontal rule when rendered, mirroring
/// PoB's mod-tooltip layout (e.g. base implicits → explicit mods →
/// crafted block).
#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub struct TooltipBody {
    pub sections: Vec<TooltipSection>,
}

/// A single section within a [`TooltipBody`].
#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub struct TooltipSection {
    /// Optional bold heading drawn above the section's lines.
    pub heading: Option<String>,
    /// Body lines. Each line may contain colour escapes.
    pub lines: Vec<String>,
}

impl TooltipBody {
    /// Build an empty body.
    pub fn new() -> Self {
        Self::default()
    }

    /// Builder: append a section with no heading.
    #[must_use]
    pub fn section(mut self, lines: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.sections.push(TooltipSection {
            heading: None,
            lines: lines.into_iter().map(Into::into).collect(),
        });
        self
    }

    /// Builder: append a section with a heading.
    #[must_use]
    pub fn section_with_heading(
        mut self,
        heading: impl Into<String>,
        lines: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.sections.push(TooltipSection {
            heading: Some(heading.into()),
            lines: lines.into_iter().map(Into::into).collect(),
        });
        self
    }

    /// Convenience: build a single-section body from a list of lines.
    pub fn from_lines(lines: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self::new().section(lines)
    }

    /// True if the body has no sections (or every section is empty).
    pub fn is_empty(&self) -> bool {
        // `Option::is_none_or` would be cleaner but stabilised in
        // 1.82, while the workspace MSRV (`Cargo.toml::rust-version`)
        // is still 1.80. Stick with `map_or(true, …)` to stay
        // compatible.
        #[allow(clippy::unnecessary_map_or)]
        self.sections
            .iter()
            .all(|s| s.heading.as_deref().map_or(true, str::is_empty) && s.lines.is_empty())
    }
}

/// Render a [`TooltipBody`] inside `ui`, applying colour-code parsing
/// to every line and a horizontal rule between sections. Intended to be
/// called from inside an `on_hover_ui` / `on_hover_ui_at_pointer`
/// closure but also works as a standalone label inside any layout.
///
/// `default_color` is the fallback for text without an explicit colour
/// escape — typically `Color32::WHITE` or the egui visuals' weak text
/// colour.
pub fn render_tooltip_body(ui: &mut egui::Ui, body: &TooltipBody, default_color: Color32) {
    let font = FontId::default();
    for (idx, section) in body.sections.iter().enumerate() {
        if idx > 0 {
            ui.separator();
        }
        if let Some(h) = &section.heading {
            if !h.is_empty() {
                let job = color_codes::to_layout_job(h, default_color, font.clone());
                ui.label(egui::RichText::new(job.text.clone()).strong());
                // Strong-styled label drops colour info; if the heading
                // contains escapes we also render the coloured layout
                // job underneath as a non-bold line for visual parity
                // with PoB's tooltip headers.
                if job.sections.len() > 1
                    || job.sections.iter().any(|s| s.format.color != default_color)
                {
                    ui.label(job);
                }
            }
        }
        for line in &section.lines {
            let job = color_codes::to_layout_job(line, default_color, font.clone());
            ui.label(job);
        }
    }
}

/// Attach a rich-content hover tooltip to `response`. Returns the
/// response so it chains naturally with other egui builders (`.clicked()`
/// etc.).
///
/// Empty bodies short-circuit: this is a no-op so callers don't need to
/// guard against "no tooltip data available" themselves.
pub fn show_rich_tooltip(
    response: Response,
    body: &TooltipBody,
    default_color: Color32,
) -> Response {
    if body.is_empty() {
        return response;
    }
    response.on_hover_ui_at_pointer(|ui| {
        render_tooltip_body(ui, body, default_color);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: PopupId = PopupId::from_static("test.a");
    const B: PopupId = PopupId::from_static("test.b");
    const C: PopupId = PopupId::from_static("test.c");

    #[test]
    fn popup_id_round_trips_static_str() {
        let id = PopupId::from_static("foo.bar");
        assert_eq!(id.as_str(), "foo.bar");
        assert_eq!(id.to_string(), "foo.bar");
    }

    #[test]
    fn popup_id_equality_is_by_static_pointer_value() {
        // Two `from_static` calls with the same literal must compare
        // equal — codegen interns string literals so the pointers
        // coincide, but assert behaviourally rather than rely on that.
        let a1 = PopupId::from_static("dup");
        let a2 = PopupId::from_static("dup");
        assert_eq!(a1, a2);
        assert_ne!(a1, PopupId::from_static("different"));
    }

    #[test]
    fn host_starts_empty() {
        let host = PopupHost::new();
        assert!(host.is_empty());
        assert_eq!(host.len(), 0);
        assert!(host.top().is_none());
    }

    #[test]
    fn open_pushes_in_lifo_order() {
        let mut host = PopupHost::new();
        host.open(PopupRequest::modal(A, "A"));
        host.open(PopupRequest::modal(B, "B"));
        host.open(PopupRequest::floating(C, "C"));
        assert_eq!(host.len(), 3);
        assert_eq!(host.top().map(|r| r.id()), Some(C));
        assert!(host.is_top(C));
        assert!(!host.is_top(A));
        let ids: Vec<_> = host.iter().map(|r| r.id()).collect();
        assert_eq!(ids, vec![A, B, C]);
    }

    #[test]
    fn re_opening_existing_id_brings_it_to_top_without_duplicating() {
        // Mirrors PoB: re-opening a dialog that's already showing
        // promotes it to the front, it does not stack a second copy.
        let mut host = PopupHost::new();
        host.open(PopupRequest::modal(A, "A"));
        host.open(PopupRequest::modal(B, "B"));
        host.open(PopupRequest::modal(A, "A again"));
        assert_eq!(host.len(), 2);
        assert!(host.is_top(A));
        // The re-pushed copy wins (so the title update is applied):
        assert_eq!(host.top().unwrap().title(), "A again");
    }

    #[test]
    fn take_top_pops_lifo() {
        let mut host = PopupHost::new();
        host.open(PopupRequest::modal(A, "A"));
        host.open(PopupRequest::modal(B, "B"));
        let popped = host.take_top().unwrap();
        assert_eq!(popped.id(), B);
        assert!(host.is_top(A));
    }

    #[test]
    fn close_by_id_removes_from_anywhere_in_stack() {
        let mut host = PopupHost::new();
        host.open(PopupRequest::modal(A, "A"));
        host.open(PopupRequest::modal(B, "B"));
        host.open(PopupRequest::modal(C, "C"));
        host.close_by_id(B);
        let ids: Vec<_> = host.iter().map(|r| r.id()).collect();
        assert_eq!(ids, vec![A, C]);
        assert!(host.is_top(C));
        assert!(!host.is_open(B));
    }

    #[test]
    fn close_by_id_no_op_when_missing() {
        let mut host = PopupHost::new();
        host.open(PopupRequest::modal(A, "A"));
        host.close_by_id(B);
        assert_eq!(host.len(), 1);
        assert!(host.is_top(A));
    }

    #[test]
    fn close_all_clears_stack() {
        let mut host = PopupHost::new();
        host.open(PopupRequest::modal(A, "A"));
        host.open(PopupRequest::modal(B, "B"));
        host.close_all();
        assert!(host.is_empty());
    }

    #[test]
    fn popup_kind_round_trips_through_request() {
        let m = PopupRequest::modal(A, "modal");
        let f = PopupRequest::floating(B, "floating");
        assert_eq!(m.kind(), PopupKind::Modal);
        assert_eq!(f.kind(), PopupKind::Floating);
    }

    #[test]
    fn request_builder_overrides_defaults() {
        let r = PopupRequest::modal(A, "A")
            .with_default_width(420.0)
            .with_default_height(300.0)
            .with_centered(false);
        assert_eq!(r.default_width, Some(420.0));
        assert_eq!(r.default_height, Some(300.0));
        assert!(!r.centered);
    }

    #[test]
    fn floating_request_defaults_off_center() {
        let r = PopupRequest::floating(A, "A");
        assert!(!r.centered);
    }

    #[test]
    fn modal_request_defaults_centered() {
        let r = PopupRequest::modal(A, "A");
        assert!(r.centered);
    }

    #[test]
    fn tooltip_body_is_empty_for_default() {
        assert!(TooltipBody::new().is_empty());
        assert!(TooltipBody::default().is_empty());
    }

    #[test]
    fn tooltip_body_from_lines_creates_single_section() {
        let body = TooltipBody::from_lines(["one", "two"]);
        assert_eq!(body.sections.len(), 1);
        assert_eq!(body.sections[0].heading, None);
        assert_eq!(body.sections[0].lines, vec!["one", "two"]);
        assert!(!body.is_empty());
    }

    #[test]
    fn tooltip_body_section_builder_chains() {
        let body = TooltipBody::new()
            .section(["base"])
            .section_with_heading("Implicit", ["+10 to all attributes"])
            .section_with_heading(
                "Explicit",
                ["+20 to maximum life", "10% increased fire damage"],
            );
        assert_eq!(body.sections.len(), 3);
        assert_eq!(body.sections[1].heading.as_deref(), Some("Implicit"));
        assert_eq!(body.sections[2].lines.len(), 2);
    }

    #[test]
    fn tooltip_body_with_only_empty_section_is_empty() {
        // A heading-less, line-less section still counts as "no
        // content", so `show_rich_tooltip` short-circuits.
        let body = TooltipBody::new().section(Vec::<String>::new());
        assert!(body.is_empty());
    }

    #[test]
    fn tooltip_body_with_heading_is_not_empty() {
        let body = TooltipBody::new().section_with_heading("Heading", Vec::<String>::new());
        assert!(!body.is_empty());
    }
}
