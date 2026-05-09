//! Issue #33 — desktop-side HTTP fetch for external build-share
//! sites (pobb.in, pastebin.com, poeplanner.com).
//!
//! Mirrors the threading shape of [`crate::ggg_fetch`]: a `std::thread`
//! per request with results delivered back to the UI through an
//! `mpsc` channel. The UI polls `try_recv` each frame so the egui
//! paint loop never blocks on the network.
//!
//! Native-only — wasm cannot reach these endpoints from the browser
//! anyway (CORS), so the wasm build path falls back to the
//! "paste the share code" flow already shipped on the Import-Export
//! tab.
//!
//! `pob_engine::resolve_share_url` does the URL → raw-endpoint mapping;
//! this module just adds the actual GET + status-code translation on
//! top.

#![cfg(not(target_arch = "wasm32"))]

use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::thread;
use std::time::Duration;

/// Why a share-URL fetch failed. Modelled to mirror the typed error
/// buckets in `crate::ggg_fetch::FetchError`, but tailored for the
/// no-auth, raw-text endpoints these build-share sites expose.
#[derive(Debug)]
pub enum ShareUrlFetchError {
    /// HTTP 404 — slug doesn't resolve. Most common failure mode (a
    /// stale or mistyped URL).
    NotFound,
    /// HTTP 429 — the site is rate-limiting our IP. Retry after a
    /// short wait. `retry_after_secs` is the `Retry-After` header
    /// value when present.
    RateLimited { retry_after_secs: Option<u64> },
    /// Anything else — network failure, unexpected status, etc.
    /// Carries the raw description so the UI can surface what went
    /// wrong without us inventing a bucket per status code.
    Other(String),
    /// Endpoint returned 200 but the body wasn't a usable PoB share
    /// code (empty, decode failure, etc.). The string is the parser
    /// error so the UI can show "decode: zlib: …" verbatim.
    Parse(String),
}

impl std::fmt::Display for ShareUrlFetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => write!(f, "Share URL not found (404)."),
            Self::RateLimited { retry_after_secs } => match retry_after_secs {
                Some(s) => write!(f, "Rate limited — try again in {s} seconds."),
                None => write!(f, "Rate limited — try again in a moment."),
            },
            Self::Other(e) => write!(f, "Network error: {e}"),
            Self::Parse(e) => write!(f, "Couldn't decode share code: {e}"),
        }
    }
}

impl std::error::Error for ShareUrlFetchError {}

/// Final result of a share-URL fetch + decode job. The character
/// payload is fully assembled by the time this lands in the UI;
/// the caller just needs to swap `*character` and recompute.
pub enum ShareUrlFetchResult {
    Ok {
        /// The decoded `Character`.
        character: pob_engine::Character,
        /// Display label for the source site (e.g. `"pobb.in"`).
        site: &'static str,
    },
    Err(ShareUrlFetchError),
}

/// In-flight fetch job — drained from the egui frame loop.
pub struct ShareUrlFetchJob {
    rx: Receiver<ShareUrlFetchResult>,
}

impl ShareUrlFetchJob {
    pub fn try_recv(&self) -> Result<Option<ShareUrlFetchResult>, ()> {
        match self.rx.try_recv() {
            Ok(r) => Ok(Some(r)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => Err(()),
        }
    }
}

/// Spawn a background thread that fetches `raw_url` and pipes the
/// response through `import_pob_code`. `raw_url` should be the
/// *resolved* endpoint produced by `pob_engine::resolve_share_url` —
/// the host-recognition logic stays in the engine crate so wasm can
/// reuse it for the URL-vs-code branch in `auto_import`.
///
/// `site` is just a display label echoed back in the success result;
/// the UI uses it for the status banner ("Imported from pobb.in.").
pub fn spawn_share_url_fetch(raw_url: String, site: &'static str) -> ShareUrlFetchJob {
    let (tx, rx) = channel();
    thread::spawn(move || {
        let result = run_share_url_fetch(&raw_url, site);
        let _ = tx.send(result);
    });
    ShareUrlFetchJob { rx }
}

/// Same fetch path, factored out so the threaded entry point and any
/// future synchronous callers (CLI, tests with a stubbed transport)
/// share one implementation.
fn run_share_url_fetch(raw_url: &str, site: &'static str) -> ShareUrlFetchResult {
    let body = match fetch_share_text(raw_url) {
        Ok(b) => b,
        Err(e) => return ShareUrlFetchResult::Err(e),
    };
    decode_share_body(&body, site)
}

/// Parse a fetched body as a PoB share code. Public so a future
/// "load build from disk" or "paste raw" flow can reuse the same
/// trim-and-decode rules.
pub fn decode_share_body(body: &str, site: &'static str) -> ShareUrlFetchResult {
    // Trim once up-front — pobb.in / pastebin both wrap the raw code
    // in trailing whitespace + a `\n` from the HTTP serializer.
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return ShareUrlFetchResult::Err(ShareUrlFetchError::Parse(
            "endpoint returned an empty body".into(),
        ));
    }
    match pob_engine::import_pob_code(trimmed) {
        Ok(character) => ShareUrlFetchResult::Ok { character, site },
        Err(e) => ShareUrlFetchResult::Err(ShareUrlFetchError::Parse(e.to_string())),
    }
}

fn fetch_share_text(url: &str) -> Result<String, ShareUrlFetchError> {
    let agent = ureq::AgentBuilder::new()
        // Mirror the GGG-fetch UA so server-side logs can attribute
        // MK2 traffic. The build-share sites don't care about the
        // exact value, but anonymous user-agents trip cloudflare on
        // pobb.in occasionally.
        .user_agent("PathOfBuildingMK2/0.0.1")
        .timeout_connect(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .build();

    match agent.get(url).call() {
        Ok(resp) => resp
            .into_string()
            .map_err(|e| ShareUrlFetchError::Other(e.to_string())),
        Err(ureq::Error::Status(code, resp)) => Err(match code {
            404 => ShareUrlFetchError::NotFound,
            429 => ShareUrlFetchError::RateLimited {
                retry_after_secs: resp
                    .header("Retry-After")
                    .and_then(|v| v.parse::<u64>().ok()),
            },
            _ => ShareUrlFetchError::Other(format!("HTTP {code}")),
        }),
        Err(ureq::Error::Transport(t)) => Err(ShareUrlFetchError::Other(t.to_string())),
    }
}

/// Extract the human-readable site label for a URL `pob_engine::resolve_share_url`
/// just successfully resolved. Used purely for the status banner.
#[must_use]
pub fn site_label_for_url(input: &str) -> &'static str {
    let lc = input.to_ascii_lowercase();
    if lc.contains("pobb.in") {
        "pobb.in"
    } else if lc.contains("pastebin.com") {
        "pastebin.com"
    } else if lc.contains("poeplanner.com") {
        "poeplanner.com"
    } else {
        "external site"
    }
}

/// Sites we recognise in `resolve_share_url` but which do *not*
/// actually expose a full-build POB-format endpoint. Right now this
/// is only `poeplanner.com` — upstream PoB imports poeplanner URLs
/// only as passive-tree links, not as full builds. We special-case
/// the recognition + clear UI message here so the user gets a
/// pointer instead of a confusing "404" error.
#[must_use]
pub fn unsupported_site(input: &str) -> Option<&'static str> {
    let lc = input.to_ascii_lowercase();
    if lc.contains("poeplanner.com") {
        Some("poeplanner.com")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_pobb_in_fixture_round_trips_through_import_pob_code() {
        // The fixture is a real exporter output served the way pobb.in's
        // `/<id>/raw` endpoint serves it: PoB share code as plain text.
        // Wrap in newlines to mimic the trailing whitespace HTTP servers
        // tend to add.
        let raw = include_str!("../../pob-engine/tests/fixtures/pobb_in_share_code.txt");
        let padded = format!("\n{raw}\n\n");
        match decode_share_body(&padded, "pobb.in") {
            ShareUrlFetchResult::Ok { site, character } => {
                assert_eq!(site, "pobb.in");
                assert!(
                    !character.class.0.is_empty(),
                    "class should be populated by import_pob_code"
                );
            }
            ShareUrlFetchResult::Err(e) => panic!("expected Ok, got {e}"),
        }
    }

    #[test]
    fn decode_pastebin_fixture_round_trips_through_import_pob_code() {
        // pastebin.com/raw/<id> serves the exact paste body verbatim.
        // The acceptance criterion is "Pasting a pastebin URL fetches
        // and imports correctly" — confirm the decode half against a
        // captured fixture so CI doesn't depend on live pastebin.
        let raw = include_str!("../../pob-engine/tests/fixtures/pastebin_share_code.txt");
        match decode_share_body(raw, "pastebin.com") {
            ShareUrlFetchResult::Ok { site, character } => {
                assert_eq!(site, "pastebin.com");
                assert!(!character.class.0.is_empty());
            }
            ShareUrlFetchResult::Err(e) => panic!("expected Ok, got {e}"),
        }
    }

    #[test]
    fn decode_garbage_body_surfaces_parse_error() {
        // Real-world failure: pobb.in has been known to 200 with an
        // HTML error page when their backend hiccups. We want a Parse
        // error rather than a panic.
        let html = "<html><body>internal error</body></html>";
        match decode_share_body(html, "pobb.in") {
            ShareUrlFetchResult::Err(ShareUrlFetchError::Parse(_)) => {}
            other => panic!(
                "expected Parse(_), got {:?}",
                match other {
                    ShareUrlFetchResult::Ok { .. } => "Ok".to_owned(),
                    ShareUrlFetchResult::Err(e) => format!("{e}"),
                }
            ),
        }
    }

    #[test]
    fn decode_share_body_rejects_empty() {
        match decode_share_body("   \n\n", "pobb.in") {
            ShareUrlFetchResult::Err(ShareUrlFetchError::Parse(m)) => {
                assert!(m.contains("empty"), "expected empty-body error, got {m}");
            }
            other => panic!(
                "expected Parse(empty), got {:?}",
                match other {
                    ShareUrlFetchResult::Ok { .. } => "Ok".to_owned(),
                    ShareUrlFetchResult::Err(e) => format!("{e}"),
                }
            ),
        }
    }

    #[test]
    fn site_label_recognises_each_host() {
        assert_eq!(site_label_for_url("https://pobb.in/abc"), "pobb.in");
        assert_eq!(
            site_label_for_url("https://pastebin.com/raw/abc"),
            "pastebin.com"
        );
        assert_eq!(
            site_label_for_url("https://poeplanner.com/build/abc"),
            "poeplanner.com"
        );
        assert_eq!(site_label_for_url("https://example.com/x"), "external site");
    }

    #[test]
    fn unsupported_site_flags_poeplanner() {
        assert_eq!(
            unsupported_site("https://poeplanner.com/Aabcd1234"),
            Some("poeplanner.com")
        );
        assert_eq!(unsupported_site("https://pobb.in/abc"), None);
        assert_eq!(unsupported_site("https://pastebin.com/raw/x"), None);
    }
}
