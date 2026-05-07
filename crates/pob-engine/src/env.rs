//! `Env` — the calc context. Maps roughly to PoB's `env` table, but Phase 2 only carries
//! what the basic-stats pass needs.

use std::collections::HashMap;

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
    pub stats: HashMap<String, f64>,
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
}
