//! Issue #194 (slice 5): wasm-side HTTP fetch for the GGG character
//! API. Mirrors the public surface of [`crate::ggg_fetch`] (the
//! desktop `ureq` path) so the import UI can call into either path
//! transparently.
//!
//! The browser routes the request through `web_sys::fetch`,
//! returning a `wasm_bindgen_futures::JsFuture` we await on the
//! main thread. There is no `std::thread`/`mpsc` scaffolding —
//! the egui frame loop polls a `Rc<RefCell<Option<Result<…>>>>`
//! that the futures populate when they resolve.
//!
//! ## CORS
//!
//! As of 2024 the GGG `pathofexile.com/character-window/*` endpoints
//! do **not** send `Access-Control-Allow-Origin: *`, so a direct
//! browser-side fetch is blocked. Two viable options:
//!
//! 1. Configure the deployed wasm app behind a reverse proxy that
//!    rewrites the request server-side (recommended; keeps the
//!    POESESSID inside the user's session cookie).
//! 2. Route through a public CORS proxy (e.g.
//!    `https://corsproxy.io/?https://www.pathofexile.com/...`).
//!    POESESSID can't safely traverse those — POE has to allow
//!    cookieless reads of the affected endpoints.
//!
//! We default to a same-origin call (option 1) by setting
//! `ggg_proxy_base()` to an empty string; deployers who want to
//! drive option 2 can override the base URL via the `GGG_PROXY_BASE`
//! build-time env var (handled by `app/pob-web/build.rs` —
//! deferred slice).

#![cfg(target_arch = "wasm32")]

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use ahash::HashMap;
use pob_engine::{
    build_character_from_ggg_with_skills, ggg_get_characters_url, ggg_get_items_url,
    ggg_get_passive_skills_url, parse_ggg_character_list, parse_ggg_items,
    parse_ggg_passive_skills, Character, GggCharacterList, GggCharacterSummary,
    GggPassiveSkillsResponse,
};

/// Pre-built `(typeLine -> skill_id)` map. Mirrors the desktop
/// `ggg_fetch::GemTypeLineMap`.
pub type GemTypeLineMap = Arc<HashMap<String, String>>;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Request, RequestCredentials, RequestInit, RequestMode, Response};

/// Why a wasm-side GGG fetch failed. Mirrors the desktop `FetchError`
/// shape so the UI doesn't need to branch.
#[derive(Debug)]
pub enum FetchError {
    Unauthorized,
    Forbidden,
    NotFound,
    RateLimited {
        retry_after_secs: Option<u64>,
    },
    /// CORS / network failure / unexpected status. The string is
    /// the message we want to surface in the UI status banner.
    Other(String),
    Parse(String),
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unauthorized => write!(f, "Sign-in is required (paste a POESESSID)."),
            Self::Forbidden => write!(f, "Account profile is private (paste a POESESSID)."),
            Self::NotFound => write!(f, "Account name not found."),
            Self::RateLimited { retry_after_secs } => match retry_after_secs {
                Some(s) => write!(f, "Rate limited — try again in {s} seconds."),
                None => write!(f, "Rate limited — try again in a moment."),
            },
            Self::Other(e) => write!(f, "Network error: {e}"),
            Self::Parse(e) => write!(f, "Couldn't parse GGG response: {e}"),
        }
    }
}

impl std::error::Error for FetchError {}

pub enum CharacterFetchResult {
    Ok {
        character: Character,
        summary: GggCharacterSummary,
        passive: GggPassiveSkillsResponse,
    },
    Err(FetchError),
}

pub enum CharacterListFetchResult {
    Ok(GggCharacterList),
    Err(FetchError),
}

/// Mirrors [`crate::ggg_fetch::CharacterFetchJob`] — a poll-able
/// handle the egui frame loop drains via `try_recv`.
pub struct CharacterFetchJob {
    slot: Rc<RefCell<Option<CharacterFetchResult>>>,
}

impl CharacterFetchJob {
    pub fn try_recv(&self) -> Result<Option<CharacterFetchResult>, ()> {
        Ok(self.slot.borrow_mut().take())
    }
}

pub struct CharacterListFetchJob {
    slot: Rc<RefCell<Option<CharacterListFetchResult>>>,
}

impl CharacterListFetchJob {
    pub fn try_recv(&self) -> Result<Option<CharacterListFetchResult>, ()> {
        Ok(self.slot.borrow_mut().take())
    }
}

/// Same-origin proxy base, e.g. `"/api"` (configured at deploy
/// time). Empty string ⇒ direct GGG URL — only works with a CORS
/// proxy in front.
fn ggg_proxy_base() -> &'static str {
    option_env!("POB_MK2_GGG_PROXY").unwrap_or("")
}

/// Wrap the upstream GGG URL in the proxy base when one is configured.
fn proxied(url: &str) -> String {
    let base = ggg_proxy_base();
    if base.is_empty() {
        url.to_owned()
    } else {
        format!("{base}{url}", url = url.trim_start_matches("https:"))
    }
}

pub fn spawn_character_list_fetch(
    account: String,
    realm: String,
    _session_id: Option<String>,
) -> CharacterListFetchJob {
    // Note: when running same-origin behind a proxy that forwards
    // the user's POESESSID cookie, we don't need to send the cookie
    // header explicitly — `credentials: include` carries it. We
    // still accept `session_id` here for API parity with the desktop
    // path but ignore it.
    let slot: Rc<RefCell<Option<CharacterListFetchResult>>> = Rc::new(RefCell::new(None));
    let slot_clone = slot.clone();
    wasm_bindgen_futures::spawn_local(async move {
        let url = proxied(&ggg_get_characters_url(&account, &realm));
        let result = match fetch_text(&url).await {
            Ok(body) => match parse_ggg_character_list(&body) {
                Ok(list) => CharacterListFetchResult::Ok(list),
                Err(e) => CharacterListFetchResult::Err(FetchError::Parse(e.to_string())),
            },
            Err(e) => CharacterListFetchResult::Err(e),
        };
        *slot_clone.borrow_mut() = Some(result);
    });
    CharacterListFetchJob { slot }
}

pub fn spawn_character_fetch(
    account: String,
    character_name: String,
    realm: String,
    _session_id: Option<String>,
    summary_hint: Option<GggCharacterSummary>,
    gem_lookup: Option<GemTypeLineMap>,
) -> CharacterFetchJob {
    let slot: Rc<RefCell<Option<CharacterFetchResult>>> = Rc::new(RefCell::new(None));
    let slot_clone = slot.clone();
    wasm_bindgen_futures::spawn_local(async move {
        let result =
            run_character_fetch(&account, &character_name, &realm, summary_hint, gem_lookup).await;
        *slot_clone.borrow_mut() = Some(result);
    });
    CharacterFetchJob { slot }
}

async fn run_character_fetch(
    account: &str,
    character_name: &str,
    realm: &str,
    summary_hint: Option<GggCharacterSummary>,
    gem_lookup: Option<GemTypeLineMap>,
) -> CharacterFetchResult {
    let passive_url = proxied(&ggg_get_passive_skills_url(account, character_name, realm));
    let items_url = proxied(&ggg_get_items_url(account, character_name, realm));

    let passive_body = match fetch_text(&passive_url).await {
        Ok(b) => b,
        Err(e) => return CharacterFetchResult::Err(e),
    };
    let passive = match parse_ggg_passive_skills(&passive_body) {
        Ok(p) => p,
        Err(e) => return CharacterFetchResult::Err(FetchError::Parse(e.to_string())),
    };

    let items_body = match fetch_text(&items_url).await {
        Ok(b) => b,
        Err(e) => return CharacterFetchResult::Err(e),
    };
    let items_resp = match parse_ggg_items(&items_body) {
        Ok(i) => i,
        Err(e) => return CharacterFetchResult::Err(FetchError::Parse(e.to_string())),
    };

    let character = build_character_from_ggg_with_skills(
        summary_hint.as_ref(),
        &passive,
        &items_resp,
        |type_line| {
            if let Some(map) = gem_lookup.as_deref() {
                if let Some(id) = map.get(&type_line.to_ascii_lowercase()) {
                    return Some(id.clone());
                }
            }
            Some(pob_engine::default_skill_id_from_type_line(type_line))
        },
    );
    let summary = summary_hint.unwrap_or_else(|| GggCharacterSummary {
        name: items_resp
            .character
            .as_ref()
            .map(|c| c.name.clone())
            .unwrap_or_else(|| character_name.to_owned()),
        class: items_resp
            .character
            .as_ref()
            .map(|c| c.class.clone())
            .unwrap_or_default(),
        class_id: items_resp.character.as_ref().and_then(|c| c.class_id),
        ascendancy_class: items_resp
            .character
            .as_ref()
            .and_then(|c| c.ascendancy_class),
        level: items_resp
            .character
            .as_ref()
            .map(|c| c.level)
            .unwrap_or(character.level),
        league: items_resp
            .character
            .as_ref()
            .map(|c| c.league.clone())
            .unwrap_or_default(),
    });
    CharacterFetchResult::Ok {
        character,
        summary,
        passive,
    }
}

/// Issue a CORS GET to `url` and return the body as a `String`.
/// HTTP error codes map to typed `FetchError` variants matching
/// the desktop path.
async fn fetch_text(url: &str) -> Result<String, FetchError> {
    let opts = RequestInit::new();
    opts.set_method("GET");
    opts.set_mode(RequestMode::Cors);
    // Send POESESSID cookie when available (the user logged into
    // the same origin in another tab). On a cross-origin direct
    // call without `Access-Control-Allow-Credentials: true` the
    // browser drops the cookie automatically — that's expected.
    opts.set_credentials(RequestCredentials::Include);
    let request = match Request::new_with_str_and_init(url, &opts) {
        Ok(r) => r,
        Err(e) => return Err(FetchError::Other(format!("bad URL: {e:?}"))),
    };
    let _ = request.headers().set("Accept", "application/json");

    let window = match web_sys::window() {
        Some(w) => w,
        None => return Err(FetchError::Other("no window".into())),
    };
    let resp_value = match JsFuture::from(window.fetch_with_request(&request)).await {
        Ok(v) => v,
        Err(e) => return Err(FetchError::Other(format!("fetch failed: {e:?}"))),
    };
    let resp: Response = match resp_value.dyn_into() {
        Ok(r) => r,
        Err(_) => return Err(FetchError::Other("response not a Response".into())),
    };
    let status = resp.status();
    if !resp.ok() {
        return Err(match status {
            401 => FetchError::Unauthorized,
            403 => FetchError::Forbidden,
            404 => FetchError::NotFound,
            429 => {
                let retry_after = resp
                    .headers()
                    .get("Retry-After")
                    .ok()
                    .flatten()
                    .and_then(|s| s.parse::<u64>().ok());
                FetchError::RateLimited {
                    retry_after_secs: retry_after,
                }
            }
            _ => FetchError::Other(format!("HTTP {status}")),
        });
    }
    let text_promise = match resp.text() {
        Ok(p) => p,
        Err(e) => return Err(FetchError::Other(format!("text() failed: {e:?}"))),
    };
    let text_value = match JsFuture::from(text_promise).await {
        Ok(v) => v,
        Err(e) => return Err(FetchError::Other(format!("await text failed: {e:?}"))),
    };
    text_value
        .as_string()
        .ok_or_else(|| FetchError::Other("body not a string".into()))
}
