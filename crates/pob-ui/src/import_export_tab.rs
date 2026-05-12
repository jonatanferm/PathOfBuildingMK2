//! Import / Export tab — generate or paste an MK2 share code, or
//! pull a character live from the GGG (Grinding Gear Games)
//! account API.

use eframe::egui;
use pob_engine::{
    export_code, export_pob_code, export_pob_xml, resolve_share_url, Character, GggCharacterSummary,
};

#[cfg(not(target_arch = "wasm32"))]
use crate::ggg_fetch::{
    spawn_character_fetch, spawn_character_list_fetch, CharacterFetchJob, CharacterFetchResult,
    CharacterListFetchJob, CharacterListFetchResult, GemTypeLineMap,
};
#[cfg(not(target_arch = "wasm32"))]
use crate::keyring_store;

#[cfg(target_arch = "wasm32")]
use crate::ggg_fetch_wasm::{
    spawn_character_fetch, spawn_character_list_fetch, CharacterFetchJob, CharacterFetchResult,
    CharacterListFetchJob, CharacterListFetchResult, GemTypeLineMap,
};
#[cfg(not(target_arch = "wasm32"))]
use crate::share_url_fetch::{
    site_label_for_url, spawn_share_url_fetch, unsupported_site, ShareUrlFetchJob,
    ShareUrlFetchResult,
};

/// Issue #194 follow-up: pick which serialisation the export pane
/// emits. Mirrors PoB's `Show Code` / `Show XML` toggle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExportFormat {
    /// `MK2|<base64>` JSON-snapshot code — our native round-trip.
    #[default]
    Mk2,
    /// `eNp…` style PoB share code (zlib + base64 of `<PathOfBuilding>` XML).
    /// Paste into upstream PoB to round-trip the build.
    PobShare,
    /// Raw `<PathOfBuilding>...` XML. Useful for diffing two builds or
    /// pasting into a script.
    PobXml,
}

impl ExportFormat {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Mk2 => "MK2 code",
            Self::PobShare => "PoB share code",
            Self::PobXml => "PoB XML",
        }
    }
}

/// Issue #194 follow-up: humanise an export code's byte length for the
/// side-of-panel size chip. Pasted PoB share codes for high-end builds
/// can comfortably exceed Discord's 2000-char message limit; surfacing
/// the size lets a user spot that *before* hitting Send.
///
/// Format conventions:
/// * Under 1024 bytes: `N B` (raw byte count).
/// * 1024 bytes and up: `K.D kB` (one decimal, rounded down — never
///   rounds *up* into the next unit so the chip can't lie about a
///   build squeezing under a quota).
///
/// Empty input yields `0 B`. Pure / no-egui so the rule is documented
/// and unit-testable in isolation.
#[must_use]
pub fn format_export_size_label(generated: &str) -> String {
    let bytes = generated.len();
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    // Truncate (don't round) so a 1535-byte code reads as "1.4 kB",
    // not "1.5 kB" — never overstate the size against a quota.
    let tenths = bytes * 10 / 1024;
    let whole = tenths / 10;
    let frac = tenths % 10;
    format!("{whole}.{frac} kB")
}

/// Issue #194 follow-up: serialise a [`Character`] in the requested
/// [`ExportFormat`]. Pure / fallible — the UI surfaces the inner error
/// as a banner. Pulled out so each branch has a unit-test home and the
/// UI side stays a thin dispatch.
pub fn export_in_format(character: &Character, format: ExportFormat) -> Result<String, String> {
    match format {
        ExportFormat::Mk2 => export_code(character).map_err(|e| format!("{e}")),
        ExportFormat::PobShare => export_pob_code(character).map_err(|e| format!("{e}")),
        ExportFormat::PobXml => Ok(export_pob_xml(character)),
    }
}

#[derive(Default)]
pub struct ImportExportTabState {
    pub paste: String,
    pub generated: String,
    pub last_message: Option<(bool, String)>,
    /// Issue #194 follow-up: which serialisation the next "Generate"
    /// click should emit. Defaults to MK2 — the historical pre-#194
    /// behaviour.
    pub export_format: ExportFormat,
    /// Issue #32 / #194: live-import inputs and in-flight job state.
    /// Available on both desktop (via `ureq`) and wasm (via
    /// `web_sys::fetch`). The browser path may need a CORS proxy
    /// — see `crate::ggg_fetch_wasm` for deployment notes.
    pub ggg: GggImportState,
    /// Issue #33: in-flight pobb.in / pastebin fetch — when a URL
    /// is pasted, the auto-import button spawns a background fetch
    /// instead of decoding inline, and we drain the result here.
    #[cfg(not(target_arch = "wasm32"))]
    pub share_url_job: Option<ShareUrlFetchJob>,
}

/// Pre-built `(gem typeLine -> canonical PoB skill_id)` lookup
/// shared across fetch jobs. Constructed once at app startup from
/// `data/gems.json` (when available) and held by the tab state so
/// every spawn picks it up without re-iterating the registry.
#[derive(Default)]
pub struct GggImportState {
    /// Account name field — accepts `Hero#1234` or `Hero-1234`.
    pub account_name: String,
    /// Realm — defaults to `pc`. PoB exposes `pc / xbox / sony`.
    pub realm: String,
    /// Optional POESESSID for private profiles. On desktop we
    /// hydrate from the OS keyring at construction time and
    /// persist to it whenever the field changes (slice 4). On
    /// wasm we don't touch the cookie ourselves — the browser
    /// already manages it via `credentials: include`.
    pub session_id: String,
    /// True when `session_id` was loaded from the keyring at
    /// construction. Tracks "should I clear the keyring entry on
    /// the next blur?" without forcing an extra prompt every
    /// keystroke. Wasm has no keyring; the field is unused there
    /// (the cookie lives in the browser).
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub session_id_loaded_from_keyring: bool,
    /// Character name. Either populated by the user manually or
    /// chosen from the dropdown after the character list fetch.
    pub character_name: String,
    /// Last successful character list — drives the dropdown.
    pub character_list: Vec<GggCharacterSummary>,
    /// In-flight character-list job.
    pub list_job: Option<CharacterListFetchJob>,
    /// In-flight import job.
    pub import_job: Option<CharacterFetchJob>,
    /// One-line status text shown under the GGG section. `Some
    /// ((ok, msg))` mirrors the rest of the tab's banner colours.
    pub status: Option<(bool, String)>,
    /// Issue #194 (slice 2): optional gem-name → canonical PoB
    /// skill_id lookup, populated from the loaded `GemSet` at app
    /// startup. Each fetch job clones the `Arc` so the spawn
    /// thread / future doesn't need to walk the registry.
    pub gem_lookup: Option<GemTypeLineMap>,
}

impl ImportExportTabState {
    /// Construct the tab state, populating the POESESSID from the
    /// OS keyring on desktop. Wasm builds skip the keyring (the
    /// cookie lives in the browser itself).
    pub fn new_with_keyring() -> Self {
        #[allow(unused_mut)]
        let mut state = Self::default();
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Some(stored) = keyring_store::load_session_id() {
                state.ggg.session_id = stored;
                state.ggg.session_id_loaded_from_keyring = true;
            }
        }
        state
    }

    /// Install a `(typeLine -> skill_id)` lookup, derived from the
    /// app's loaded `GemSet`. The tab clones the `Arc` into each
    /// fetch job so gem name resolution lands the canonical PoB
    /// skill id (e.g. `"Spell Echo Support"` →
    /// `"SupportSpellEcho"`) instead of the engine's permissive
    /// fallback.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub fn set_gem_lookup(&mut self, lookup: GemTypeLineMap) {
        self.ggg.gem_lookup = Some(lookup);
    }
}

pub fn ui(
    ui: &mut egui::Ui,
    state: &mut ImportExportTabState,
    character: &mut Character,
    tree: &pob_data::PassiveTree,
) -> bool {
    let mut changed = false;

    // Drain any in-flight jobs first so the panel below renders the freshest status.
    // GGG fetch works on desktop (ureq + std::thread) and wasm (web_sys::fetch +
    // spawn_local) — both routes share the same poll-once-per-frame pattern (#194 slice 5).
    if poll_ggg_jobs(&mut state.ggg, character, tree) {
        changed = true;
    }
    // Share-URL fetch (pobb.in / pastebin) is desktop-only — CORS blocks the
    // browser path. See #202.
    #[cfg(not(target_arch = "wasm32"))]
    {
        if poll_share_url_job(state, character) {
            changed = true;
        }
    }

    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_min_width(360.0);
            ui.heading("Export current build");
            ui.separator();
            // Issue #194 follow-up: format dropdown + single Generate
            // button replaces the previous per-format buttons. Adds
            // a "PoB XML" option for users who want the raw
            // `<PathOfBuilding>` document (diffing, scripting).
            ui.horizontal(|ui| {
                ui.label("Format:");
                egui::ComboBox::from_id_salt("export_format_select")
                    .selected_text(state.export_format.label())
                    .show_ui(ui, |ui| {
                        for fmt in [
                            ExportFormat::Mk2,
                            ExportFormat::PobShare,
                            ExportFormat::PobXml,
                        ] {
                            ui.selectable_value(&mut state.export_format, fmt, fmt.label());
                        }
                    });
                if ui.button("Generate").clicked() {
                    match export_in_format(character, state.export_format) {
                        Ok(code) => {
                            state.generated = code;
                            state.last_message =
                                Some((true, format!("Generated {}.", state.export_format.label())));
                        }
                        Err(e) => {
                            state.last_message = Some((false, format!("Export failed: {e}")));
                        }
                    }
                }
            });
            ui.add(
                egui::TextEdit::multiline(&mut state.generated)
                    .desired_width(f32::INFINITY)
                    .desired_rows(8)
                    .font(egui::TextStyle::Monospace),
            );
            if !state.generated.is_empty() {
                ui.horizontal(|ui| {
                    if ui.button("Copy code to clipboard").clicked() {
                        ui.ctx().copy_text(state.generated.clone());
                        state.last_message = Some((true, "Copied to clipboard.".into()));
                    }
                    // Mirror the Import side's Clear affordance so the
                    // user can drop a generated code without
                    // hand-selecting the textarea.
                    if ui.button("Clear").clicked() {
                        state.generated.clear();
                        state.last_message = None;
                    }
                    // Size chip: high-end PoB share codes routinely
                    // exceed Discord's 2000-char paste limit; surfacing
                    // the byte count lets the user spot that before
                    // hitting Send.
                    ui.weak(format_export_size_label(&state.generated));
                });
            }
        });

        ui.separator();

        ui.vertical(|ui| {
            ui.set_min_width(360.0);
            ui.heading("Import build");
            ui.separator();
            ui.label("Paste an MK2 build code or share URL:");
            ui.add(
                egui::TextEdit::multiline(&mut state.paste)
                    .desired_width(f32::INFINITY)
                    .desired_rows(8)
                    .font(egui::TextStyle::Monospace)
                    .hint_text("MK2|...   or   https://pobb.in/<id>"),
            );
            #[cfg(not(target_arch = "wasm32"))]
            let fetch_in_flight = state.share_url_job.is_some();
            #[cfg(target_arch = "wasm32")]
            let fetch_in_flight = false;
            ui.horizontal(|ui| {
                let import_btn =
                    ui.add_enabled(!fetch_in_flight, egui::Button::new("Import (auto)"));
                if import_btn.clicked() {
                    handle_import_click(state, character, &mut changed);
                }
                if ui.button("Clear").clicked() {
                    state.paste.clear();
                    state.last_message = None;
                }
                // Live "detected: <kind>" chip next to the Import
                // button. Lets the user see whether the import will
                // route to the MK2 / XML / share-code decoder, or
                // spawn a URL fetch, before clicking Import.
                if let Some(kind) = detect_paste_format(&state.paste) {
                    ui.weak(format!("Detected: {kind}"));
                }
            });
            ui.weak(
                "Auto-detects MK2 codes, raw PoB XML, PoB share codes (zlib+base64), \
                 or share URLs from pobb.in / pob.cool / pastebin.com / poe.ninja.",
            );
        });
    });

    if let Some((ok, msg)) = &state.last_message {
        ui.add_space(4.0);
        let colour = if *ok {
            egui::Color32::LIGHT_GREEN
        } else {
            egui::Color32::LIGHT_RED
        };
        ui.colored_label(colour, msg);
    }

    // Issue #32 / #194: live character import section. Available
    // on desktop and wasm — wasm needs a CORS-capable proxy
    // configured via the `POB_MK2_GGG_PROXY` env var at build
    // time (see `ggg_fetch_wasm.rs` for deployment notes).
    ui.add_space(8.0);
    ui.separator();
    if ggg_section(ui, &mut state.ggg, character) {
        changed = true;
    }

    changed
}

/// Dispatch the "Import (auto)" button click. Three cases:
///
/// 1. The pasted text is a recognised share URL → spawn a background
///    fetch (native only) and surface the result via `poll_share_url_job`.
/// 2. The pasted text is a recognised share URL on a host that doesn't
///    expose a POB-format raw endpoint (poeplanner.com today) → tell the
///    user to paste the code directly. No network call.
/// 3. Otherwise → run the existing in-process `auto_import` (MK2 code,
///    raw XML, or PoB share code).
///
/// The wasm build skips spawning entirely (no `std::thread`, CORS
/// blocked) and reports a "desktop only" hint instead.
fn handle_import_click(
    state: &mut ImportExportTabState,
    character: &mut Character,
    changed: &mut bool,
) {
    let trimmed = state.paste.trim();
    if trimmed.is_empty() {
        state.last_message = Some((false, "Nothing to import — paste a code or URL.".into()));
        return;
    }
    // URL branch — same recognition logic on native and wasm so the
    // user gets a consistent message in both, but only native can
    // actually run the fetch.
    if let Some(raw_url) = resolve_share_url(trimmed) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Some(host) = unsupported_site(trimmed) {
                state.last_message = Some((
                    false,
                    format!(
                        "{host} doesn't expose a full-build POB export. \
                         Open the build there, copy the PoB code from the page, \
                         and paste it here.",
                    ),
                ));
                return;
            }
            let label = site_label_for_url(trimmed);
            state.share_url_job = Some(spawn_share_url_fetch(raw_url, label));
            state.last_message = Some((true, format!("Fetching from {label}…")));
            return;
        }
        #[cfg(target_arch = "wasm32")]
        {
            // The browser build can't reach pobb.in / pastebin
            // directly (CORS). Mirror the GGG-section wording.
            let _ = raw_url;
            state.last_message = Some((
                false,
                "Build-share URL fetch is desktop-only for now \
                 (browser CORS blocks the raw endpoints). \
                 Open the share link, copy the PoB code, paste it here."
                    .to_owned(),
            ));
            return;
        }
    }
    // Not a URL — fall through to in-process decode.
    match auto_import(trimmed) {
        Ok((c, kind)) => {
            *character = c;
            state.last_message = Some((true, format!("Imported as {kind}.")));
            state.paste.clear();
            *changed = true;
        }
        Err(e) => {
            state.last_message = Some((false, format!("Import failed: {e}")));
        }
    }
}

/// Issue #194 follow-up: a guess at what the user just pasted, used
/// to surface a small chip next to the textarea so they can see
/// whether the input will land where they expect *before* clicking
/// Import. Mirrors the format-tier order [`auto_import`] tries, plus
/// the share-URL branch [`handle_import_click`] short-circuits to a
/// network fetch on.
///
/// Returns `None` when the buffer is empty / whitespace-only — the
/// caller suppresses the chip in that case so the cold-open path is
/// unchanged.
///
/// Pure / no-egui so the detection rule is documented and
/// unit-testable in isolation.
#[must_use]
pub fn detect_paste_format(input: &str) -> Option<&'static str> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    // URL takes priority over the prefix branches — a pasted
    // pobb.in URL goes through the share-fetch path, not the
    // in-process MK2 / XML decoders.
    if resolve_share_url(trimmed).is_some() {
        return Some("Share URL");
    }
    if trimmed.starts_with("MK2|") {
        return Some("MK2 code");
    }
    if trimmed.starts_with('<') {
        return Some("PoB XML");
    }
    // Long opaque text is most likely a zlib+base64 PoB share code.
    // The threshold of 30 is well below the smallest realistic share
    // code (an empty character clocks in at ~150 chars) so the
    // false-positive rate against random pastes is low; below it,
    // assume the user is mid-typing and keep the chip quiet.
    if trimmed.len() > 30 {
        return Some("PoB share code (?)");
    }
    None
}

/// Try the formats in order of specificity. Delegates the actual
/// decoding to [`crate::compare_tab::import_build_text`] (which owns
/// the canonical prefix-sniff rule) and attaches a format-label tag
/// for the post-import status message. A successful decode confirms
/// the format, so the label drops the speculative `(?)` suffix that
/// [`detect_paste_format`] uses for un-validated input.
fn auto_import(input: &str) -> Result<(Character, &'static str), String> {
    let character = crate::compare_tab::import_build_text(input)?;
    let trimmed = input.trim();
    let kind = if trimmed.starts_with("MK2|") {
        "MK2 code"
    } else if trimmed.starts_with('<') {
        "PoB XML"
    } else {
        "PoB share code"
    };
    Ok((character, kind))
}

fn ggg_section(ui: &mut egui::Ui, state: &mut GggImportState, _character: &mut Character) -> bool {
    ui.heading("Import from PoE account (live)");
    ui.weak(
        "Pulls a character directly from the GGG character-window API. \
         Public profiles work without credentials; private profiles need a POESESSID.",
    );
    ui.add_space(4.0);

    let in_flight = state.list_job.is_some() || state.import_job.is_some();

    egui::Grid::new("ggg_import_form")
        .num_columns(2)
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            ui.label("Account name:");
            ui.add_enabled(
                !in_flight,
                egui::TextEdit::singleline(&mut state.account_name)
                    .hint_text("Hero#1234")
                    .desired_width(220.0),
            );
            ui.end_row();

            ui.label("Realm:");
            let realms = ["pc", "xbox", "sony"];
            if state.realm.is_empty() {
                state.realm = "pc".into();
            }
            egui::ComboBox::from_id_salt("ggg_realm")
                .selected_text(state.realm.clone())
                .show_ui(ui, |ui| {
                    for r in realms {
                        ui.selectable_value(&mut state.realm, r.to_owned(), r);
                    }
                });
            ui.end_row();

            ui.label("POESESSID (optional):");
            ui.horizontal(|ui| {
                ui.add_enabled(
                    !in_flight,
                    egui::TextEdit::singleline(&mut state.session_id)
                        .password(true)
                        .hint_text("32-char hex; needed for private profiles")
                        .desired_width(220.0),
                );
                #[cfg(not(target_arch = "wasm32"))]
                {
                    if ui
                        .add_enabled(
                            !in_flight && !state.session_id.trim().is_empty(),
                            egui::Button::new("Save"),
                        )
                        .on_hover_text("Persist this POESESSID to the OS keyring (Keychain / Credential Manager / secret-service) so you don't have to re-paste it next session.")
                        .clicked()
                    {
                        match keyring_store::save_session_id(state.session_id.trim()) {
                            Ok(()) => {
                                state.session_id_loaded_from_keyring = true;
                                state.status = Some((true, "POESESSID saved to OS keyring.".to_owned()));
                            }
                            Err(e) => {
                                state.status = Some((false, format!("Couldn't save POESESSID: {e}")));
                            }
                        }
                    }
                    if ui
                        .add_enabled(
                            !in_flight && state.session_id_loaded_from_keyring,
                            egui::Button::new("Forget"),
                        )
                        .on_hover_text("Remove the saved POESESSID from the OS keyring.")
                        .clicked()
                    {
                        match keyring_store::clear_session_id() {
                            Ok(()) => {
                                state.session_id.clear();
                                state.session_id_loaded_from_keyring = false;
                                state.status = Some((true, "POESESSID cleared.".to_owned()));
                            }
                            Err(e) => {
                                state.status = Some((false, format!("Couldn't clear POESESSID: {e}")));
                            }
                        }
                    }
                }
            });
            ui.end_row();
        });

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        if ui
            .add_enabled(
                !in_flight && !state.account_name.trim().is_empty(),
                egui::Button::new("Fetch character list"),
            )
            .clicked()
        {
            let session = if state.session_id.trim().is_empty() {
                None
            } else {
                Some(state.session_id.trim().to_owned())
            };
            state.list_job = Some(spawn_character_list_fetch(
                state.account_name.trim().to_owned(),
                state.realm.clone(),
                session,
            ));
            state.status = Some((true, "Retrieving character list…".to_owned()));
            state.character_list.clear();
        }

        if !state.character_list.is_empty() {
            let selected_label = if state.character_name.is_empty() {
                "(pick a character)".to_owned()
            } else {
                state.character_name.clone()
            };
            egui::ComboBox::from_id_salt("ggg_character_picker")
                .selected_text(selected_label)
                .width(220.0)
                .show_ui(ui, |ui| {
                    for char_summary in &state.character_list {
                        let detail = format!(
                            "{} — {} lvl {} ({})",
                            char_summary.name,
                            if char_summary.class.is_empty() {
                                "?"
                            } else {
                                char_summary.class.as_str()
                            },
                            char_summary.level,
                            if char_summary.league.is_empty() {
                                "?"
                            } else {
                                char_summary.league.as_str()
                            },
                        );
                        ui.selectable_value(
                            &mut state.character_name,
                            char_summary.name.clone(),
                            detail,
                        );
                    }
                });
        } else {
            ui.add(
                egui::TextEdit::singleline(&mut state.character_name)
                    .hint_text("Character name (case-sensitive)")
                    .desired_width(220.0),
            );
        }

        if ui
            .add_enabled(
                !in_flight
                    && !state.character_name.trim().is_empty()
                    && !state.account_name.trim().is_empty(),
                egui::Button::new("Import character"),
            )
            .clicked()
        {
            let session = if state.session_id.trim().is_empty() {
                None
            } else {
                Some(state.session_id.trim().to_owned())
            };
            // Carry the matching list entry as a hint so the
            // resulting character keeps its `class` even when the
            // items endpoint returns an empty envelope.
            let summary_hint = state
                .character_list
                .iter()
                .find(|c| c.name == state.character_name.trim())
                .cloned();
            state.import_job = Some(spawn_character_fetch(
                state.account_name.trim().to_owned(),
                state.character_name.trim().to_owned(),
                state.realm.clone(),
                session,
                summary_hint,
                state.gem_lookup.clone(),
            ));
            state.status = Some((true, "Fetching character data…".to_owned()));
        }
    });

    if let Some((ok, msg)) = &state.status {
        ui.add_space(4.0);
        let colour = if *ok {
            egui::Color32::LIGHT_GREEN
        } else {
            egui::Color32::LIGHT_RED
        };
        ui.colored_label(colour, msg);
    }
    // Reports `false` from this fn — the actual `character`
    // mutation happens in `poll_ggg_jobs` which runs at the top of
    // every frame.
    false
}

/// Drain any in-flight GGG jobs. Returns `true` when this frame's
/// drain produced a new active character, signalling to the parent
/// app that a recompute is needed. Desktop uses background threads
/// through an `mpsc` channel; wasm uses `wasm_bindgen_futures`
/// dropping into a `Rc<RefCell>`. We poll-once-per-frame so
/// repeated UI redraws don't pile up duplicate spawns.
fn poll_ggg_jobs(
    state: &mut GggImportState,
    character: &mut Character,
    tree: &pob_data::PassiveTree,
) -> bool {
    let mut character_changed = false;

    // Character-list job.
    if let Some(job) = state.list_job.as_ref() {
        match job.try_recv() {
            Ok(Some(CharacterListFetchResult::Ok(list))) => {
                let count = list.len();
                state.character_list = list;
                if let Some(first) = state.character_list.first() {
                    state.character_name = first.name.clone();
                }
                state.status = Some((
                    true,
                    format!("Retrieved {count} character(s). Pick one to import."),
                ));
                state.list_job = None;
            }
            Ok(Some(CharacterListFetchResult::Err(err))) => {
                state.status = Some((false, err.to_string()));
                state.list_job = None;
            }
            Ok(None) => {}
            Err(_) => {
                state.status = Some((false, "Background fetch thread disconnected.".into()));
                state.list_job = None;
            }
        }
    }

    // Character-import job.
    if let Some(job) = state.import_job.as_ref() {
        match job.try_recv() {
            Ok(Some(CharacterFetchResult::Ok {
                character: imported,
                summary,
                passive,
            })) => {
                *character = imported;
                // Issue #194 (slice 3): wire passive-tree jewels
                // (cluster + radius + abyss + timeless) into the
                // character via the live tree. Cluster jewels land
                // on `character.jewels`, others on
                // `character.socketed_jewels`.
                let jewel_count = pob_engine::apply_ggg_passive_jewels(character, tree, &passive);
                state.status = Some((
                    true,
                    if jewel_count > 0 {
                        format!(
                            "Imported '{}' ({} lvl {}, {} tree jewels).",
                            summary.name,
                            if summary.class.is_empty() {
                                "?"
                            } else {
                                summary.class.as_str()
                            },
                            summary.level,
                            jewel_count,
                        )
                    } else {
                        format!(
                            "Imported '{}' ({} lvl {}).",
                            summary.name,
                            if summary.class.is_empty() {
                                "?"
                            } else {
                                summary.class.as_str()
                            },
                            summary.level,
                        )
                    },
                ));
                state.import_job = None;
                character_changed = true;
            }
            Ok(Some(CharacterFetchResult::Err(err))) => {
                state.status = Some((false, err.to_string()));
                state.import_job = None;
            }
            Ok(None) => {}
            Err(_) => {
                state.status = Some((false, "Background fetch thread disconnected.".into()));
                state.import_job = None;
            }
        }
    }

    character_changed
}

/// Issue #33: drain the in-flight share-URL fetch (pobb.in / pastebin)
/// and apply the resulting `Character` to the active build. Returns
/// `true` when this frame's drain swapped the character so the host
/// app knows to recompute.
#[cfg(not(target_arch = "wasm32"))]
fn poll_share_url_job(state: &mut ImportExportTabState, character: &mut Character) -> bool {
    let Some(job) = state.share_url_job.as_ref() else {
        return false;
    };
    match job.try_recv() {
        Ok(Some(ShareUrlFetchResult::Ok {
            character: imported,
            site,
        })) => {
            *character = imported;
            state.last_message = Some((true, format!("Imported from {site}.")));
            state.paste.clear();
            state.share_url_job = None;
            true
        }
        Ok(Some(ShareUrlFetchResult::Err(err))) => {
            state.last_message = Some((false, format!("Import failed: {err}")));
            state.share_url_job = None;
            false
        }
        Ok(None) => false,
        Err(_) => {
            state.last_message = Some((false, "Background fetch thread disconnected.".into()));
            state.share_url_job = None;
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pob_engine::ClassRef;

    fn fixture_character() -> Character {
        let mut c = Character::new(ClassRef::ranger(), 92);
        c.notes = "round-trip fixture".into();
        c
    }

    #[test]
    fn export_in_format_default_is_mk2() {
        // Default state should pick the same format the pre-#194 button
        // pair emitted first — `MK2|<base64>`.
        assert_eq!(ExportFormat::default(), ExportFormat::Mk2);
    }

    #[test]
    fn export_in_format_mk2_emits_mk2_prefix() {
        // The MK2 share format always carries the `MK2|` prefix —
        // confirms the dispatch hit the right engine helper.
        let code = export_in_format(&fixture_character(), ExportFormat::Mk2).expect("export ok");
        assert!(
            code.starts_with("MK2|"),
            "MK2 format should produce the documented prefix, got {code:?}",
        );
    }

    #[test]
    fn export_in_format_pob_xml_emits_pathofbuilding_root() {
        // PoB XML format is the raw `<PathOfBuilding>` document —
        // diff-friendly and what users paste into scripts.
        let xml = export_in_format(&fixture_character(), ExportFormat::PobXml).expect("export ok");
        assert!(
            xml.contains("<PathOfBuilding"),
            "PoB XML format should produce a <PathOfBuilding> document, got {xml:?}",
        );
    }

    #[test]
    fn export_in_format_pob_share_emits_non_empty_zlib_blob() {
        // PoB share code is opaque base64 — at minimum it should be
        // non-empty for a well-formed character.
        let code =
            export_in_format(&fixture_character(), ExportFormat::PobShare).expect("export ok");
        assert!(!code.is_empty());
        // PoB share codes don't carry the `MK2|` prefix.
        assert!(!code.starts_with("MK2|"));
    }

    #[test]
    fn export_format_label_is_user_friendly() {
        // The combo dropdown reads the labels — pin them so a typo
        // doesn't quietly land in the UI.
        assert_eq!(ExportFormat::Mk2.label(), "MK2 code");
        assert_eq!(ExportFormat::PobShare.label(), "PoB share code");
        assert_eq!(ExportFormat::PobXml.label(), "PoB XML");
    }

    // ─── auto_import ─────────────────────────────────────────────────────

    #[test]
    fn auto_import_round_trips_mk2_code_with_correct_label() {
        // MK2 round-trip — the kind label is what the status message
        // shows the user post-import ("Imported as MK2 code.").
        let original = fixture_character();
        let code = export_in_format(&original, ExportFormat::Mk2).expect("export");
        let (back, kind) = auto_import(&code).expect("auto_import ok");
        assert_eq!(kind, "MK2 code");
        assert_eq!(back.class.0, original.class.0);
    }

    #[test]
    fn auto_import_round_trips_pob_xml_with_correct_label() {
        let original = fixture_character();
        let xml = export_in_format(&original, ExportFormat::PobXml).expect("export");
        let (_back, kind) = auto_import(&xml).expect("auto_import ok");
        assert_eq!(kind, "PoB XML");
    }

    #[test]
    fn auto_import_round_trips_pob_share_code_with_correct_label() {
        // PoB share code is the fall-through branch — confirms the
        // label is the unparenthesised "PoB share code" (no `(?)`
        // suffix) since a successful decode validates the format.
        let original = fixture_character();
        let share = export_in_format(&original, ExportFormat::PobShare).expect("export");
        let (_back, kind) = auto_import(&share).expect("auto_import ok");
        assert_eq!(kind, "PoB share code");
    }

    #[test]
    fn auto_import_surfaces_empty_input_error_through_import_build_text() {
        // Delegated empty-input error from `import_build_text` should
        // pass through unchanged so the status banner reads the
        // friendly message rather than an opaque decode failure.
        let err = auto_import("").expect_err("empty input is an error");
        assert!(err.contains("Nothing to import"));
    }

    // ─── format_export_size_label ────────────────────────────────────────

    #[test]
    fn size_label_uses_raw_bytes_under_one_kilobyte() {
        // Bytes are easier to interpret than fractional kB at small
        // sizes — and "0 B" is the natural empty-state.
        assert_eq!(format_export_size_label(""), "0 B");
        assert_eq!(format_export_size_label("hello"), "5 B");
        // 1023 is still in the raw-byte band — boundary check so a
        // future refactor doesn't accidentally flip the threshold.
        let near_boundary = "a".repeat(1023);
        assert_eq!(format_export_size_label(&near_boundary), "1023 B");
    }

    #[test]
    fn size_label_switches_to_kilobytes_at_one_thousand_twenty_four() {
        // Exactly 1024 bytes is "1.0 kB" — the threshold is inclusive
        // on the kB side so we don't flicker between units near 1 kB.
        let exactly_kb = "a".repeat(1024);
        assert_eq!(format_export_size_label(&exactly_kb), "1.0 kB");
    }

    #[test]
    fn size_label_truncates_rather_than_rounds_at_kilobyte_scale() {
        // 1535 bytes is 1.499… kB — the chip MUST read "1.4 kB", not
        // "1.5 kB", so a build that would actually fail a 1.5 kB
        // quota can't sneak by with a rounded-down chip. Mirrors the
        // doc comment's "never lie about squeezing under a quota"
        // contract.
        let mid_kb = "a".repeat(1535);
        assert_eq!(format_export_size_label(&mid_kb), "1.4 kB");
    }

    #[test]
    fn size_label_handles_multi_kilobyte_codes() {
        // 10 kB-class codes are common for high-end PoB share output —
        // make sure the format stays "K.D kB" without thousands
        // separators or scientific notation.
        let ten_kb = "a".repeat(10 * 1024);
        assert_eq!(format_export_size_label(&ten_kb), "10.0 kB");
    }

    // ─── detect_paste_format ─────────────────────────────────────────────

    #[test]
    fn detect_paste_format_returns_none_for_empty_or_whitespace() {
        // Cold-open path: chip is suppressed when the buffer is empty
        // (or whitespace-only — trim semantics match the import path).
        assert_eq!(detect_paste_format(""), None);
        assert_eq!(detect_paste_format("   "), None);
        assert_eq!(detect_paste_format("\n\t"), None);
    }

    #[test]
    fn detect_paste_format_identifies_mk2_prefix() {
        // The MK2 branch fires on the literal prefix — no length
        // gate, because a short MK2 code is technically possible
        // (an empty character ends up under ~200 chars but still
        // valid).
        assert_eq!(detect_paste_format("MK2|abc"), Some("MK2 code"));
        // Leading whitespace doesn't break detection — the import
        // path trims first, so the chip must too.
        assert_eq!(detect_paste_format("  MK2|xyz"), Some("MK2 code"));
    }

    #[test]
    fn detect_paste_format_identifies_xml_prefix() {
        assert_eq!(
            detect_paste_format("<PathOfBuilding><Build /></PathOfBuilding>"),
            Some("PoB XML")
        );
    }

    #[test]
    fn detect_paste_format_identifies_share_urls_before_prefixes() {
        // A URL takes priority — pobb.in URLs would otherwise fall
        // through to the share-code branch because they're "long
        // opaque text". The chip must mirror what the click handler
        // actually does (spawn a fetch).
        assert_eq!(
            detect_paste_format("https://pobb.in/abc123"),
            Some("Share URL")
        );
    }

    #[test]
    fn detect_paste_format_identifies_long_text_as_share_code() {
        // Long opaque text with no recognised prefix is the share-code
        // branch. The "?" suffix signals that the format is a guess —
        // we can't validate the zlib+base64 contents without actually
        // decoding.
        let long = "abcdefghijklmnopqrstuvwxyz0123456789==";
        assert_eq!(detect_paste_format(long), Some("PoB share code (?)"));
    }

    #[test]
    fn detect_paste_format_keeps_quiet_for_short_random_text() {
        // Below the share-code length threshold: probably mid-typing.
        // Suppress the chip rather than spuriously labelling it as a
        // share code.
        assert_eq!(detect_paste_format("hello world"), None);
    }
}
