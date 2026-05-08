//! `Env` — the calc context. Maps roughly to PoB's `env` table, but Phase 2 only carries
//! what the basic-stats pass needs.

use ahash::AHashMap;

use crate::mod_db::{EvalState, ModDB};

#[derive(Debug, Default)]
pub struct Env {
    pub mod_db: ModDB,
    pub state: EvalState,
    pub output: Output,
}

/// Flat output dictionary. Mirrors `env.player.output` in PoB.
#[derive(Debug, Default, Clone)]
pub struct Output {
    pub stats: AHashMap<String, f64>,
}

impl Output {
    pub fn set(&mut self, name: impl Into<String>, value: f64) {
        self.stats.insert(name.into(), value);
    }
    pub fn get(&self, name: &str) -> f64 {
        self.stats.get(name).copied().unwrap_or(0.0)
    }
    pub fn try_get(&self, name: &str) -> Option<f64> {
        self.stats.get(name).copied()
    }
    pub fn iter(&self) -> impl Iterator<Item = (&str, f64)> {
        self.stats.iter().map(|(k, v)| (k.as_str(), *v))
    }
    pub fn len(&self) -> usize {
        self.stats.len()
    }
    pub fn is_empty(&self) -> bool {
        self.stats.is_empty()
    }
    /// Insert under `prefix + suffix` without going through the `format!()`
    /// machinery. Saves both the formatter overhead and an intermediate alloc
    /// in the perform-pass hot loops.
    #[inline]
    pub fn set_concat(&mut self, prefix: &str, suffix: &str, value: f64) {
        let mut s = String::with_capacity(prefix.len() + suffix.len());
        s.push_str(prefix);
        s.push_str(suffix);
        self.stats.insert(s, value);
    }
    /// Read under `prefix + suffix`. Uses a small stack buffer for short keys
    /// so the lookup never allocates; falls back to a heap String for keys
    /// longer than the buffer.
    #[inline]
    pub fn get_concat(&self, prefix: &str, suffix: &str) -> f64 {
        let total = prefix.len() + suffix.len();
        if total <= 64 {
            let mut buf = [0u8; 64];
            buf[..prefix.len()].copy_from_slice(prefix.as_bytes());
            buf[prefix.len()..total].copy_from_slice(suffix.as_bytes());
            // Both halves were valid UTF-8 strings, so the concatenation is too.
            // The `from_utf8` check is fast and (for ASCII keys) keeps the lint clean.
            let key = std::str::from_utf8(&buf[..total]).expect("valid utf-8 join");
            self.stats.get(key).copied().unwrap_or(0.0)
        } else {
            let mut s = String::with_capacity(total);
            s.push_str(prefix);
            s.push_str(suffix);
            self.stats.get(&s).copied().unwrap_or(0.0)
        }
    }
}
