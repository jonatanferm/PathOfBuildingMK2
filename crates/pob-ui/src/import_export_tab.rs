//! Import / Export tab — generate or paste an MK2 share code, or
//! pull a character live from the GGG (Grinding Gear Games)
//! account API.

use eframe::egui;
use pob_engine::{
    export_code, export_pob_code, import_code, import_pob_code, import_pob_xml, resolve_share_url,
    Character,
};

#[cfg(not(target_arch = "wasm32"))]
use pob_engine::GggCharacterSummary;

#[cfg(not(target_arch = "wasm32"))]
use crate::ggg_fetch::{
    spawn_character_fetch, spawn_character_list_fetch, CharacterFetchJob, CharacterFetchResult,
    CharacterListFetchJob, CharacterListFetchResult,
};
#[cfg(not(target_arch = "wasm32"))]
use crate::share_url_fetch::{
    site_label_for_url, spawn_share_url_fetch, unsupported_site, ShareUrlFetchJob,
    ShareUrlFetchResult,
};

#[derive(Default)]
pub struct ImportExportTabState {
    pub paste: String,
    pub generated: String,
    pub last_message: Option<(bool, String)>,
    /// Issue #32: live-import inputs and in-flight job state. Wasm
    /// has different network constraints (CORS, no `std::thread`),
    /// so the UI half is also gated; the user sees a "desktop only"
    /// stub there.
    #[cfg(not(target_arch = "wasm32"))]
    pub ggg: GggImportState,
    /// Issue #33: in-flight pobb.in / pastebin fetch — when a URL
    /// is pasted, the auto-import button spawns a background fetch
    /// instead of decoding inline, and we drain the result here.
    #[cfg(not(target_arch = "wasm32"))]
    pub share_url_job: Option<ShareUrlFetchJob>,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
pub struct GggImportState {
    /// Account name field — accepts `Hero#1234` or `Hero-1234`.
    pub account_name: String,
    /// Realm — defaults to `pc`. PoB exposes `pc / xbox / sony`.
    pub realm: String,
    /// Optional POESESSID for private profiles. Stored in memory
    /// only; not persisted to disk yet (see issue body for the
    /// cache-in-keyring follow-up).
    pub session_id: String,
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
}

pub fn ui(ui: &mut egui::Ui, state: &mut ImportExportTabState, character: &mut Character) -> bool {
    let mut changed = false;

    // Drain any in-flight GGG jobs first so the panel below renders
    // the freshest status. On wasm this is a no-op stub.
    #[cfg(not(target_arch = "wasm32"))]
    {
        if poll_ggg_jobs(&mut state.ggg, character) {
            changed = true;
        }
        if poll_share_url_job(state, character) {
            changed = true;
        }
    }

    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_min_width(360.0);
            ui.heading("Export current build");
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Generate MK2 code").clicked() {
                    match export_code(character) {
                        Ok(code) => {
                            state.generated = code;
                            state.last_message = Some((true, "Generated MK2 code.".into()));
                        }
                        Err(e) => {
                            state.last_message = Some((false, format!("Export failed: {e}")));
                        }
                    }
                }
                if ui.button("Generate PoB share code").clicked() {
                    match export_pob_code(character) {
                        Ok(code) => {
                            state.generated = code;
                            state.last_message = Some((
                                true,
                                "Generated PoB-compatible code (paste into upstream PoB to round-trip).".into(),
                            ));
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
            if !state.generated.is_empty() && ui.button("Copy code to clipboard").clicked() {
                ui.ctx().copy_text(state.generated.clone());
                state.last_message = Some((true, "Copied to clipboard.".into()));
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
                let import_btn = ui.add_enabled(
                    !fetch_in_flight,
                    egui::Button::new("Import (auto)"),
                );
                if import_btn.clicked() {
                    handle_import_click(state, character, &mut changed);
                }
                if ui.button("Clear").clicked() {
                    state.paste.clear();
                    state.last_message = None;
                }
            });
            ui.weak(
                "Auto-detects MK2 codes, raw PoB XML, PoB share codes (zlib+base64), \
                 or share URLs from pobb.in / pastebin.com.",
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

    // Issue #32: live character import section. Native-only — wasm
    // gets a brief explanatory stub.
    ui.add_space(8.0);
    ui.separator();
    #[cfg(not(target_arch = "wasm32"))]
    {
        if ggg_section(ui, &mut state.ggg, character) {
            changed = true;
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        ui.heading("Import from PoE account");
        ui.weak(
            "Live character import via the GGG character-window API is desktop-only for now. \
             The browser can't reach the endpoint without CORS support.",
        );
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

/// Try the formats in order of specificity.
fn auto_import(input: &str) -> Result<(Character, &'static str), String> {
    let trimmed = input.trim();
    if trimmed.starts_with("MK2|") {
        return import_code(trimmed)
            .map(|c| (c, "MK2 code"))
            .map_err(|e| e.to_string());
    }
    if trimmed.starts_with('<') {
        return import_pob_xml(trimmed)
            .map(|c| (c, "PoB XML"))
            .map_err(|e| e.to_string());
    }
    // Fall through to PoB share code (zlib+base64).
    import_pob_code(trimmed)
        .map(|c| (c, "PoB share code"))
        .map_err(|e| e.to_string())
}

#[cfg(not(target_arch = "wasm32"))]
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
            ui.add_enabled(
                !in_flight,
                egui::TextEdit::singleline(&mut state.session_id)
                    .password(true)
                    .hint_text("32-char hex; needed for private profiles")
                    .desired_width(320.0),
            );
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
/// app that a recompute is needed. Background threads send their
/// result through an `mpsc` channel; we poll-once-per-frame so
/// repeated UI redraws don't pile up duplicate spawns.
#[cfg(not(target_arch = "wasm32"))]
fn poll_ggg_jobs(state: &mut GggImportState, character: &mut Character) -> bool {
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
            })) => {
                *character = imported;
                state.status = Some((
                    true,
                    format!(
                        "Imported '{}' ({} lvl {}).",
                        summary.name,
                        if summary.class.is_empty() {
                            "?"
                        } else {
                            summary.class.as_str()
                        },
                        summary.level,
                    ),
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
