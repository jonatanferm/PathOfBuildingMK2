//! Issue #32 — desktop-side HTTP fetch for the GGG character API.
//!
//! Native targets only — wasm uses a different fetch path (the
//! browser's `fetch` API, with the same JSON parsers from
//! [`pob_engine::ggg_import`]). The HTTP work runs on a background
//! thread; the UI polls `try_recv` each frame to drain results
//! without blocking the egui paint loop.

#![cfg(not(target_arch = "wasm32"))]

use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::thread;
use std::time::Duration;

use pob_engine::{
    build_character_from_ggg, ggg_get_characters_url, ggg_get_items_url,
    ggg_get_passive_skills_url, parse_ggg_character_list, parse_ggg_items,
    parse_ggg_passive_skills, Character, GggCharacterList, GggCharacterSummary, GggImportError,
};

/// Why a GGG fetch failed. Mirrors the user-facing buckets PoB
/// surfaces in `ImportTab.lua:459-475`.
#[derive(Debug)]
pub enum FetchError {
    /// HTTP 401 — sign-in required (POESESSID missing or expired).
    Unauthorized,
    /// HTTP 403 — account profile is private.
    Forbidden,
    /// HTTP 404 — account name is incorrect.
    NotFound,
    /// HTTP 429 — rate limited. The `retry_after_secs` field is the
    /// `Retry-After` header value when present (else `None`); the
    /// UI surfaces "try again in N seconds" when set.
    RateLimited { retry_after_secs: Option<u64> },
    /// Anything else — network failure, unexpected status, etc.
    Other(String),
    /// JSON shape didn't match the GGG endpoints (or the response
    /// body was `false` / empty).
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

impl From<GggImportError> for FetchError {
    fn from(e: GggImportError) -> Self {
        Self::Parse(e.to_string())
    }
}

/// Final result of an "import character" job — either a fully-built
/// `Character` (with its summary kept for status messages) or a
/// typed error.
pub enum CharacterFetchResult {
    Ok {
        character: Character,
        summary: GggCharacterSummary,
    },
    Err(FetchError),
}

/// Result of a "list characters on an account" job.
pub enum CharacterListFetchResult {
    Ok(GggCharacterList),
    Err(FetchError),
}

/// In-flight fetch job — the receiver is `try_recv`'d each frame.
pub struct CharacterFetchJob {
    rx: Receiver<CharacterFetchResult>,
}

impl CharacterFetchJob {
    pub fn try_recv(&self) -> Result<Option<CharacterFetchResult>, ()> {
        match self.rx.try_recv() {
            Ok(r) => Ok(Some(r)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => Err(()),
        }
    }
}

pub struct CharacterListFetchJob {
    rx: Receiver<CharacterListFetchResult>,
}

impl CharacterListFetchJob {
    pub fn try_recv(&self) -> Result<Option<CharacterListFetchResult>, ()> {
        match self.rx.try_recv() {
            Ok(r) => Ok(Some(r)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => Err(()),
        }
    }
}

/// Spawn a background thread that fetches the character list for an
/// account. Returns immediately with a job handle the caller polls.
pub fn spawn_character_list_fetch(
    account: String,
    realm: String,
    session_id: Option<String>,
) -> CharacterListFetchJob {
    let (tx, rx) = channel();
    thread::spawn(move || {
        let url = ggg_get_characters_url(&account, &realm);
        let result = match fetch_text(&url, session_id.as_deref()) {
            Ok(body) => match parse_ggg_character_list(&body) {
                Ok(list) => CharacterListFetchResult::Ok(list),
                Err(e) => CharacterListFetchResult::Err(FetchError::Parse(e.to_string())),
            },
            Err(e) => CharacterListFetchResult::Err(e),
        };
        let _ = tx.send(result);
    });
    CharacterListFetchJob { rx }
}

/// Spawn a background thread that fetches passive-skills + items
/// for the named character and assembles a [`Character`].
pub fn spawn_character_fetch(
    account: String,
    character_name: String,
    realm: String,
    session_id: Option<String>,
    summary_hint: Option<GggCharacterSummary>,
) -> CharacterFetchJob {
    let (tx, rx) = channel();
    thread::spawn(move || {
        let result = run_character_fetch(
            &account,
            &character_name,
            &realm,
            session_id.as_deref(),
            summary_hint,
        );
        let _ = tx.send(result);
    });
    CharacterFetchJob { rx }
}

fn run_character_fetch(
    account: &str,
    character_name: &str,
    realm: &str,
    session_id: Option<&str>,
    summary_hint: Option<GggCharacterSummary>,
) -> CharacterFetchResult {
    let passive_url = ggg_get_passive_skills_url(account, character_name, realm);
    let items_url = ggg_get_items_url(account, character_name, realm);

    let passive_body = match fetch_text(&passive_url, session_id) {
        Ok(b) => b,
        Err(e) => return CharacterFetchResult::Err(e),
    };
    let passive = match parse_ggg_passive_skills(&passive_body) {
        Ok(p) => p,
        Err(e) => return CharacterFetchResult::Err(FetchError::Parse(e.to_string())),
    };

    let items_body = match fetch_text(&items_url, session_id) {
        Ok(b) => b,
        Err(e) => return CharacterFetchResult::Err(e),
    };
    let items_resp = match parse_ggg_items(&items_body) {
        Ok(i) => i,
        Err(e) => return CharacterFetchResult::Err(FetchError::Parse(e.to_string())),
    };

    let character = build_character_from_ggg(summary_hint.as_ref(), &passive, &items_resp);
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
    CharacterFetchResult::Ok { character, summary }
}

/// Issue a single `ureq` GET, attaching the optional POESESSID
/// cookie + an Anthropic-friendly user agent. Maps HTTP error codes
/// to typed `FetchError`s.
fn fetch_text(url: &str, session_id: Option<&str>) -> Result<String, FetchError> {
    let agent = ureq::AgentBuilder::new()
        // PoB advertises a custom UA; we mirror that so GGG can
        // identify MK2 traffic in their telemetry.
        .user_agent("PathOfBuildingMK2/0.0.1")
        // Total timeouts — generous enough for GGG's 1+s typical
        // response time, tight enough that a stalled request
        // doesn't keep the background thread alive forever.
        .timeout_connect(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .build();
    let mut req = agent.get(url);
    if let Some(sid) = session_id {
        if !sid.is_empty() {
            req = req.set("Cookie", &format!("POESESSID={sid}"));
        }
    }

    match req.call() {
        Ok(resp) => resp
            .into_string()
            .map_err(|e| FetchError::Other(e.to_string())),
        Err(ureq::Error::Status(code, resp)) => Err(match code {
            401 => FetchError::Unauthorized,
            403 => FetchError::Forbidden,
            404 => FetchError::NotFound,
            429 => FetchError::RateLimited {
                retry_after_secs: resp
                    .header("Retry-After")
                    .and_then(|v| v.parse::<u64>().ok()),
            },
            _ => FetchError::Other(format!("HTTP {code}")),
        }),
        Err(ureq::Error::Transport(t)) => Err(FetchError::Other(t.to_string())),
    }
}
