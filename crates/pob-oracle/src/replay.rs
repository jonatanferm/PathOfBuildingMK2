//! Replay of not-yet-ported callees (calc doc §9.2, "Replay").
//!
//! Owner: ORC-X. Stub created by U0 with the final signatures: a `Replay`
//! swaps a callee (PF-G calling `offence`, OF-A calling skill hooks,
//! triggers calling `buildActiveSkill`, …) for its recorded `in → out`
//! state delta, keyed by hit id (`offence:in#4` → `offence:out#4`).

use std::collections::BTreeMap;

use crate::schema::StateDump;

/// Recorded callee deltas for one build run.
#[derive(Default)]
pub struct Replay {
    /// hit id of the callee's `:in` → its `:out` dump.
    pub deltas: BTreeMap<String, StateDump>,
}

impl Replay {
    /// Load the recorded `:in`/`:out` pairs of `callee` (a function probe id) for a build.
    pub fn load(callee: &str, dumps: &BTreeMap<String, StateDump>) -> Replay {
        let _ = (callee, dumps);
        pob_calc::unported!("replay::Replay::load (ORC-X)")
    }

    /// Apply the delta recorded for `hit` to the live Rust state (returns false
    /// when the hit was not recorded).
    pub fn apply(&self, hit: &str, env: &pob_calc::env::EnvRef) -> bool {
        let _ = (hit, env);
        pob_calc::unported!("replay::Replay::apply (ORC-X)")
    }
}
