//! Issue #194 (slice 4): persist the user's POESESSID across
//! sessions via the OS-native keyring (Keychain / Credential
//! Manager / secret-service).
//!
//! The session token authenticates GGG character-window requests
//! for private profiles. We deliberately don't fall back to a
//! plaintext file when the keyring is unavailable — a missing
//! token simply means the user has to re-paste, matching the
//! pre-keyring behaviour and avoiding a footgun where the file
//! winds up in a backup or git-tracked dotfile.
//!
//! Native-only — wasm has its own surface (the browser holds the
//! cookie-jar; we just delegate to `web_sys::fetch` with
//! `credentials: include`).

#![cfg(not(target_arch = "wasm32"))]

const SERVICE: &str = "pob-mk2";
const USER: &str = "ggg-poesessid";

/// Read the saved POESESSID, if any. `None` covers all failure
/// modes — entry not found, OS keyring locked, platform without a
/// backend — so the caller treats them identically.
pub fn load_session_id() -> Option<String> {
    let entry = keyring::Entry::new(SERVICE, USER).ok()?;
    let value = entry.get_password().ok()?;
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

/// Store `session_id` in the OS keyring under `(pob-mk2,
/// ggg-poesessid)`. An empty string is treated as a delete request
/// (mirrors the UI's "clear" button).
///
/// Returns `Err(_)` only when the OS keyring rejected the write.
/// The caller should still consider the in-memory value
/// authoritative for the session — failure here just means
/// next launch will re-prompt.
pub fn save_session_id(session_id: &str) -> Result<(), String> {
    let entry = keyring::Entry::new(SERVICE, USER).map_err(|e| e.to_string())?;
    if session_id.is_empty() {
        // `keyring::Error::NoEntry` is a no-op success when we
        // wanted to clear an entry that wasn't there to begin with.
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    } else {
        entry.set_password(session_id).map_err(|e| e.to_string())
    }
}

/// Convenience helper for the UI's "forget my session" button.
pub fn clear_session_id() -> Result<(), String> {
    save_session_id("")
}
