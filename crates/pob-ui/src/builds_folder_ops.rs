//! Issue #213 (slice 3): pure helpers for folder operations on the
//! Builds tab — name validation and unique-name suffixing.
//!
//! Slice 1 (#375) shipped the [`crate::builds_folder_tree`] data
//! layer. Slice 2 (#380) shipped the renderer. This slice provides
//! the pure logic the next renderer slice will wire into a
//! right-click context menu ("New folder…", "Rename folder…",
//! "Delete folder"). Keeping the validation and uniqueness rules
//! pure (no egui, no filesystem) means we can tighten them under
//! tests without paying the cost of an end-to-end folder-mutation
//! round-trip.
//!
//! Validation mirrors the platform restrictions PoB inherits from
//! the host filesystem: we forbid the `/` and `\` directory
//! separators, leading dots (so users can't accidentally create
//! hidden folders that the rescan walker would silently drop — see
//! `build_store_disk::rescan`), the Windows-reserved characters
//! `<>:"|?*`, and trailing whitespace / dots (Windows truncates
//! these on `CreateFile`, which would otherwise produce two folders
//! that look identical in the tree but live at different paths).
//!
//! Uniqueness suffixing matches the convention `duplicate_target`
//! already uses for files: try the bare name, then `<name> (2)`,
//! `<name> (3)`, … up to a sanity cap. The caller passes a
//! `name_exists` predicate so we stay filesystem-free.
//!
//! Nothing in the renderer wires these helpers yet — that's the
//! next slice (right-click context menu on a folder header). The
//! `dead_code` allow keeps the build warning-clean until then; the
//! tests below pin the contract so the renderer slice can land
//! without revisiting the rules.

#![allow(dead_code)]

use std::fmt;

/// Reasons a candidate folder name is rejected by
/// [`validate_folder_name`]. The display impl renders a
/// user-facing message suitable for an inline form error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FolderNameError {
    /// Name is empty or whitespace-only.
    Empty,
    /// Name contains a path separator (`/` or `\`).
    ContainsSeparator,
    /// Name contains a Windows-reserved character (`<>:"|?*`) or an
    /// ASCII control byte.
    ReservedCharacter(char),
    /// Name begins with `.` — would create a hidden directory the
    /// rescan walker drops.
    LeadingDot,
    /// Name ends in whitespace or `.` — Windows silently strips
    /// these, producing aliasing issues across platforms.
    TrailingWhitespaceOrDot,
    /// Name matches a Windows reserved device name (CON, PRN, AUX,
    /// NUL, COM1-9, LPT1-9), case-insensitive, with or without an
    /// extension.
    ReservedDeviceName,
    /// Name exceeds [`MAX_FOLDER_NAME_LEN`] bytes.
    TooLong,
}

impl fmt::Display for FolderNameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "Folder name cannot be empty."),
            Self::ContainsSeparator => {
                write!(f, "Folder name cannot contain '/' or '\\'.")
            }
            Self::ReservedCharacter(c) => {
                write!(f, "Folder name cannot contain '{c}'.")
            }
            Self::LeadingDot => write!(f, "Folder name cannot start with '.'."),
            Self::TrailingWhitespaceOrDot => {
                write!(f, "Folder name cannot end with whitespace or '.'.")
            }
            Self::ReservedDeviceName => {
                write!(f, "Folder name is a reserved system name.")
            }
            Self::TooLong => write!(f, "Folder name is too long (max 255 bytes)."),
        }
    }
}

impl std::error::Error for FolderNameError {}

/// Maximum byte length we accept for a folder name. Picked to stay
/// safely under the 255-byte limit most filesystems impose on a
/// single path component (ext4, APFS, NTFS), with no separate
/// allowance for a `(N)` suffix because suffixed candidates only
/// add a few bytes.
pub const MAX_FOLDER_NAME_LEN: usize = 255;

/// Validate a candidate folder name and return the trimmed form.
///
/// Trims leading/trailing ASCII whitespace before the per-character
/// checks — trailing whitespace itself is rejected only after the
/// trim returns the remaining content (so `"Levelling "` becomes
/// `"Levelling"` and is accepted, while `"Levelling. "` rejects on
/// the trailing dot inside the trimmed value).
pub fn validate_folder_name(name: &str) -> Result<String, FolderNameError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(FolderNameError::Empty);
    }
    if trimmed.len() > MAX_FOLDER_NAME_LEN {
        return Err(FolderNameError::TooLong);
    }
    if trimmed.starts_with('.') {
        return Err(FolderNameError::LeadingDot);
    }
    if trimmed.ends_with('.') || trimmed.ends_with(char::is_whitespace) {
        return Err(FolderNameError::TrailingWhitespaceOrDot);
    }
    for c in trimmed.chars() {
        if c == '/' || c == '\\' {
            return Err(FolderNameError::ContainsSeparator);
        }
        if matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*') || (c as u32) < 0x20 {
            return Err(FolderNameError::ReservedCharacter(c));
        }
    }
    if is_reserved_device_name(trimmed) {
        return Err(FolderNameError::ReservedDeviceName);
    }
    Ok(trimmed.to_owned())
}

fn is_reserved_device_name(name: &str) -> bool {
    // Compare against the part before the first '.', case-insensitive.
    let stem = name.split('.').next().unwrap_or(name);
    let upper = stem.to_ascii_uppercase();
    matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (upper.len() == 4
            && (upper.starts_with("COM") || upper.starts_with("LPT"))
            && upper
                .as_bytes()
                .last()
                .map(|b| b.is_ascii_digit() && *b != b'0')
                .unwrap_or(false))
}

/// Return a unique folder name based on `base`, suffixing `(2)`,
/// `(3)`, … until `name_exists` returns `false`.
///
/// `base` is *not* re-validated — call [`validate_folder_name`]
/// first if the input came from the user. The caller-supplied
/// predicate lets this helper stay filesystem-free; tests pass a
/// `HashSet`, the real renderer will pass a closure that walks
/// the sibling folder list.
///
/// Caps at 999 attempts so a runaway predicate (always returns
/// `true`) doesn't wedge the UI; in that pathological case we
/// return `<base> (999)` and let the caller decide whether to
/// surface a "couldn't find a free name" error.
#[must_use]
pub fn format_unique_folder_name<F>(base: &str, mut name_exists: F) -> String
where
    F: FnMut(&str) -> bool,
{
    if !name_exists(base) {
        return base.to_owned();
    }
    for n in 2..=999 {
        let candidate = format!("{base} ({n})");
        if !name_exists(&candidate) {
            return candidate;
        }
    }
    format!("{base} (999)")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    // ----- validate_folder_name -----

    #[test]
    fn accepts_simple_name() {
        assert_eq!(validate_folder_name("Levelling").unwrap(), "Levelling");
    }

    #[test]
    fn trims_surrounding_whitespace() {
        assert_eq!(
            validate_folder_name("  Bossing  ").unwrap(),
            "Bossing",
            "leading/trailing whitespace should be stripped, not rejected",
        );
    }

    #[test]
    fn rejects_empty() {
        assert_eq!(validate_folder_name(""), Err(FolderNameError::Empty));
    }

    #[test]
    fn rejects_whitespace_only() {
        assert_eq!(validate_folder_name("   "), Err(FolderNameError::Empty));
        assert_eq!(validate_folder_name("\t\n"), Err(FolderNameError::Empty));
    }

    #[test]
    fn rejects_forward_slash() {
        assert_eq!(
            validate_folder_name("Lev/Mar"),
            Err(FolderNameError::ContainsSeparator),
        );
    }

    #[test]
    fn rejects_backslash() {
        assert_eq!(
            validate_folder_name("Lev\\Mar"),
            Err(FolderNameError::ContainsSeparator),
        );
    }

    #[test]
    fn rejects_leading_dot() {
        // Hidden folder — rescan would silently drop the contents.
        assert_eq!(
            validate_folder_name(".cache"),
            Err(FolderNameError::LeadingDot),
        );
    }

    #[test]
    fn rejects_trailing_dot() {
        // Windows truncates trailing dots on CreateFile, causing
        // cross-platform aliasing.
        assert_eq!(
            validate_folder_name("Levelling."),
            Err(FolderNameError::TrailingWhitespaceOrDot),
        );
    }

    #[test]
    fn rejects_internal_whitespace_only_at_end() {
        // Internal whitespace is fine; trailing-after-trim is not
        // (but leading/trailing whitespace gets trimmed first, so
        // this can only happen for a name that contains a trailing
        // dot followed by whitespace — covered by the trailing-dot
        // test — or other trailing whitespace once trimmed empties
        // it). Internal spaces are explicitly allowed.
        assert_eq!(
            validate_folder_name("Hard Core Bossing").unwrap(),
            "Hard Core Bossing",
        );
    }

    #[test]
    fn rejects_windows_reserved_characters() {
        for c in ['<', '>', ':', '"', '|', '?', '*'] {
            let name = format!("bad{c}name");
            assert_eq!(
                validate_folder_name(&name),
                Err(FolderNameError::ReservedCharacter(c)),
                "char {c:?} should be rejected",
            );
        }
    }

    #[test]
    fn rejects_control_characters() {
        let name = "bad\x01name";
        assert_eq!(
            validate_folder_name(name),
            Err(FolderNameError::ReservedCharacter('\x01')),
        );
    }

    #[test]
    fn rejects_reserved_device_names_case_insensitive() {
        for name in ["CON", "con", "PRN", "AUX", "NUL", "COM1", "LPT9"] {
            assert_eq!(
                validate_folder_name(name),
                Err(FolderNameError::ReservedDeviceName),
                "{name} should be rejected as a reserved device name",
            );
        }
    }

    #[test]
    fn rejects_reserved_device_names_with_extension() {
        // "CON.txt" is also reserved on Windows — the device name
        // matches the stem.
        assert_eq!(
            validate_folder_name("CON.txt"),
            Err(FolderNameError::ReservedDeviceName),
        );
    }

    #[test]
    fn allows_names_starting_with_reserved_prefix() {
        // "CONFIG", "COMET", "AUXILIARY" are NOT reserved — the
        // device-name match is exact (modulo extension).
        assert_eq!(validate_folder_name("CONFIG").unwrap(), "CONFIG");
        assert_eq!(validate_folder_name("COMET").unwrap(), "COMET");
        assert_eq!(validate_folder_name("AUXILIARY").unwrap(), "AUXILIARY");
        // COM0 and LPT0 are not reserved (only COM1-9 / LPT1-9).
        assert_eq!(validate_folder_name("COM0").unwrap(), "COM0");
    }

    #[test]
    fn rejects_too_long() {
        let long = "a".repeat(MAX_FOLDER_NAME_LEN + 1);
        assert_eq!(validate_folder_name(&long), Err(FolderNameError::TooLong),);
    }

    #[test]
    fn accepts_at_max_length() {
        let max = "a".repeat(MAX_FOLDER_NAME_LEN);
        assert_eq!(
            validate_folder_name(&max).unwrap().len(),
            MAX_FOLDER_NAME_LEN
        );
    }

    #[test]
    fn accepts_unicode_names() {
        // Non-ASCII letters are fine — the only character bans are
        // the Windows-reserved ASCII set plus separators.
        assert_eq!(validate_folder_name("Lévelling").unwrap(), "Lévelling");
        assert_eq!(validate_folder_name("ボス").unwrap(), "ボス");
    }

    #[test]
    fn error_display_is_human_readable() {
        // Smoke-check the Display impl so callers can wire it
        // straight into an inline form-error label.
        assert_eq!(
            FolderNameError::Empty.to_string(),
            "Folder name cannot be empty.",
        );
        assert!(FolderNameError::ContainsSeparator.to_string().contains('/'));
        assert!(FolderNameError::ReservedCharacter('?')
            .to_string()
            .contains('?'));
    }

    // ----- format_unique_folder_name -----

    #[test]
    fn unique_name_returns_base_when_free() {
        let used: HashSet<String> = HashSet::new();
        assert_eq!(
            format_unique_folder_name("Levelling", |n| used.contains(n)),
            "Levelling",
        );
    }

    #[test]
    fn unique_name_appends_suffix_2_on_first_collision() {
        let used: HashSet<String> = ["Levelling".to_owned()].into_iter().collect();
        assert_eq!(
            format_unique_folder_name("Levelling", |n| used.contains(n)),
            "Levelling (2)",
        );
    }

    #[test]
    fn unique_name_skips_existing_suffixed_variants() {
        let used: HashSet<String> = ["Lev", "Lev (2)", "Lev (3)"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        assert_eq!(
            format_unique_folder_name("Lev", |n| used.contains(n)),
            "Lev (4)",
        );
    }

    #[test]
    fn unique_name_skips_to_first_gap() {
        // (2) is taken but (3) is free — the gap is filled even
        // though (4), (5), … are also taken further down. The first
        // gap wins.
        let used: HashSet<String> = ["Lev", "Lev (2)", "Lev (4)", "Lev (5)"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        assert_eq!(
            format_unique_folder_name("Lev", |n| used.contains(n)),
            "Lev (3)",
        );
    }

    #[test]
    fn unique_name_caps_at_999_when_predicate_always_true() {
        // Pathological predicate — should not loop forever.
        let result = format_unique_folder_name("Boom", |_| true);
        assert_eq!(result, "Boom (999)");
    }

    #[test]
    fn unique_name_predicate_sees_each_candidate_once() {
        // Document the call pattern: the predicate is invoked with
        // `base` first, then `<base> (2)`, `<base> (3)`, …
        let mut seen: Vec<String> = Vec::new();
        let used: HashSet<String> = ["x", "x (2)"].into_iter().map(str::to_owned).collect();
        let result = format_unique_folder_name("x", |candidate| {
            seen.push(candidate.to_owned());
            used.contains(candidate)
        });
        assert_eq!(result, "x (3)");
        assert_eq!(seen, vec!["x", "x (2)", "x (3)"]);
    }
}
