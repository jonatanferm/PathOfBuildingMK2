//! User-facing preferences for [#225](https://github.com/jonatanferm/games/PathOfBuildingMK2/issues/225).
//!
//! This is the minimum-viable settings surface — two prefs today (UI
//! scale + toast lifetime), with shape designed to grow without
//! breaking the on-disk format. Mirrors the persistence pattern from
//! [`crate::shared_items`]: JSON under the platform data dir,
//! in-memory fallback when the disk path can't be resolved or wasm.
//!
//! Out of scope for this slice: dpi multiplier, default colours,
//! separator chars, save folder override, last-used league — all
//! tracked in the parent issue as follow-up slices.

use serde::{Deserialize, Serialize};
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;

/// User preferences. Each field has a `serde(default)` plus a manual
/// `Default` impl so a settings file authored by an older app version
/// loads cleanly when this struct grows new fields.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UserSettings {
    /// UI scale factor applied via `egui::Context::set_pixels_per_point`.
    /// 1.0 is the default native pixel ratio. Clamped to `0.5..=2.5` on
    /// load so a corrupted value can't shrink the window to nothing or
    /// blow it past the screen.
    #[serde(default = "default_ui_scale")]
    pub ui_scale: f32,
    /// Lifetime of a toast notification, in seconds. PoB's default is
    /// 5; we expose the knob so users running long sessions can fade
    /// the overlay faster (or leave messages up longer for screen
    /// readers / shoulder-surfing). Clamped to `1.0..=30.0` on load.
    #[serde(default = "default_toast_lifetime")]
    pub toast_lifetime_secs: f64,
    /// Issue #225: visual theme. Dark by default (matches PoB's
    /// in-game palette). Light is provided for users on bright
    /// monitors / outdoor laptops; egui ships built-in palettes for
    /// both so the toggle is a one-liner at the apply site.
    #[serde(default = "default_theme")]
    pub theme: Theme,
}

/// Issue #225: theme palette options. egui supplies both built-in;
/// `apply` wires the right `Visuals` onto a context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    /// `egui::Visuals::dark()` — PoB's traditional palette.
    #[default]
    Dark,
    /// `egui::Visuals::light()` — bright backgrounds, dark text.
    Light,
}

impl Theme {
    /// Human-readable label for the radio button.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Dark => "Dark",
            Self::Light => "Light",
        }
    }
}

fn default_theme() -> Theme {
    Theme::Dark
}

fn default_ui_scale() -> f32 {
    1.0
}

fn default_toast_lifetime() -> f64 {
    5.0
}

impl Default for UserSettings {
    fn default() -> Self {
        Self {
            ui_scale: default_ui_scale(),
            toast_lifetime_secs: default_toast_lifetime(),
            theme: default_theme(),
        }
    }
}

impl UserSettings {
    /// Hard clamps applied at load time so a hand-edited or truncated
    /// settings file can't put the app in an unusable state. Pure /
    /// no I/O so tests can pin the rules.
    pub fn sanitised(mut self) -> Self {
        self.ui_scale = self.ui_scale.clamp(0.5, 2.5);
        self.toast_lifetime_secs = self.toast_lifetime_secs.clamp(1.0, 30.0);
        self
    }

    /// Parse JSON into a settings struct, applying [`Self::sanitised`].
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        let raw: Self = serde_json::from_str(json)?;
        Ok(raw.sanitised())
    }

    /// Serialise to a pretty-printed JSON string. Lossless;
    /// `from_json(self.to_json()?) == self` for any sanitised input.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}

/// Issue #225: load settings from the platform data dir, falling back
/// to the [`Default`] on any failure (missing file, parse error,
/// missing data dir). Mirrors `shared_items::load_from_disk` — we
/// never error from the caller's perspective; the worst case is
/// "user starts with defaults".
#[cfg(not(target_arch = "wasm32"))]
#[must_use]
pub fn load_from_disk() -> UserSettings {
    let Some(path) = settings_path() else {
        return UserSettings::default();
    };
    let Ok(json) = std::fs::read_to_string(&path) else {
        return UserSettings::default();
    };
    UserSettings::from_json(&json).unwrap_or_default()
}

/// Write the supplied settings to the platform data dir. Returns
/// `Err` only on actual I/O failure (so the host can surface a
/// status toast); a missing data dir is treated as
/// success-without-disk.
#[cfg(not(target_arch = "wasm32"))]
pub fn save_to_disk(settings: &UserSettings) -> std::io::Result<()> {
    let Some(path) = settings_path() else {
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = settings
        .to_json()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(&path, json)
}

/// Resolve the on-disk settings file. Returns `None` when the data
/// dir can't be located (CI sandbox etc.) — callers fall back to
/// in-memory only.
#[cfg(not(target_arch = "wasm32"))]
#[must_use]
pub fn settings_path() -> Option<PathBuf> {
    let mut p = data_dir()?;
    p.push("settings.json");
    Some(p)
}

#[cfg(not(target_arch = "wasm32"))]
fn data_dir() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var_os("HOME")?;
        let mut p = PathBuf::from(home);
        p.push("Library");
        p.push("Application Support");
        p.push("PathOfBuildingMK2");
        Some(p)
    }
    #[cfg(target_os = "linux")]
    {
        let base = std::env::var_os("XDG_DATA_HOME").map_or_else(
            || {
                std::env::var_os("HOME").map(|h| {
                    let mut p = PathBuf::from(h);
                    p.push(".local");
                    p.push("share");
                    p
                })
            },
            |x| Some(PathBuf::from(x)),
        )?;
        let mut p = base;
        p.push("PathOfBuildingMK2");
        Some(p)
    }
    #[cfg(target_os = "windows")]
    {
        let appdata = std::env::var_os("APPDATA")?;
        let mut p = PathBuf::from(appdata);
        p.push("PathOfBuildingMK2");
        Some(p)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_values_match_documented_constants() {
        let s = UserSettings::default();
        assert!((s.ui_scale - 1.0).abs() < f32::EPSILON);
        assert!((s.toast_lifetime_secs - 5.0).abs() < f64::EPSILON);
    }

    #[test]
    fn sanitised_clamps_ui_scale() {
        // Tiny scale (corrupted file) snaps to the min so the user
        // can still read the dialog and fix it.
        let tiny = UserSettings {
            ui_scale: 0.01,
            toast_lifetime_secs: 5.0,
            theme: Theme::Dark,
        };
        assert!((tiny.sanitised().ui_scale - 0.5).abs() < f32::EPSILON);
        // Huge scale snaps to the max.
        let huge = UserSettings {
            ui_scale: 100.0,
            toast_lifetime_secs: 5.0,
            theme: Theme::Dark,
        };
        assert!((huge.sanitised().ui_scale - 2.5).abs() < f32::EPSILON);
    }

    #[test]
    fn sanitised_clamps_toast_lifetime() {
        let zero = UserSettings {
            ui_scale: 1.0,
            toast_lifetime_secs: 0.0,
            theme: Theme::Dark,
        };
        assert!((zero.sanitised().toast_lifetime_secs - 1.0).abs() < f64::EPSILON);
        let huge = UserSettings {
            ui_scale: 1.0,
            toast_lifetime_secs: 1_000.0,
            theme: Theme::Dark,
        };
        assert!((huge.sanitised().toast_lifetime_secs - 30.0).abs() < f64::EPSILON);
    }

    #[test]
    fn from_json_applies_clamps() {
        // A hand-rolled JSON with an out-of-range scale loads
        // through `from_json` clamped — no separate sanitisation
        // call needed at the call site.
        let json = r#"{"ui_scale": 5.0, "toast_lifetime_secs": 0.1}"#;
        let s = UserSettings::from_json(json).expect("parse");
        assert!((s.ui_scale - 2.5).abs() < f32::EPSILON);
        assert!((s.toast_lifetime_secs - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn from_json_serde_defaults_cover_missing_fields() {
        // A settings file authored by a previous app version that
        // didn't have `toast_lifetime_secs` / `theme` loads with the
        // defaults — the user doesn't lose their existing prefs when
        // upgrading to a version that grew new ones.
        let partial = r#"{"ui_scale": 1.25}"#;
        let s = UserSettings::from_json(partial).expect("parse");
        assert!((s.ui_scale - 1.25).abs() < f32::EPSILON);
        assert!((s.toast_lifetime_secs - 5.0).abs() < f64::EPSILON);
        assert_eq!(s.theme, Theme::Dark);
    }

    #[test]
    fn round_trip_through_json_preserves_values() {
        let s = UserSettings {
            ui_scale: 1.5,
            toast_lifetime_secs: 8.0,
            theme: Theme::Light,
        };
        let json = s.to_json().expect("serialise");
        let back = UserSettings::from_json(&json).expect("parse");
        assert_eq!(back, s);
    }

    #[test]
    fn theme_serialises_as_lowercase_string() {
        // Pin the on-disk encoding so hand-edited settings.json
        // remains readable. `rename_all = "lowercase"` plus the
        // enum variant names mean Dark/Light serialise as "dark"
        // / "light" — both directions.
        let s = UserSettings {
            ui_scale: 1.0,
            toast_lifetime_secs: 5.0,
            theme: Theme::Light,
        };
        let json = s.to_json().expect("serialise");
        assert!(
            json.contains("\"theme\": \"light\""),
            "expected lowercase theme key, got {json}"
        );
        let parsed = UserSettings::from_json(&json).expect("parse");
        assert_eq!(parsed.theme, Theme::Light);
    }
}
