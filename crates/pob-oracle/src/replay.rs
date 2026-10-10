//! Replay of not-yet-ported callees (calc doc §9.2, "Replay").
//!
//! Owner: ORC-X. A [`Replay`] swaps a callee (PF-G calling `offence`, OF-A
//! calling skill hooks through `runSkillFunc`, triggers calling
//! `buildActiveSkill`, …) for its recorded `in → out` state, keyed by hit id
//! (`offence:in#4` → `offence:out#4`; a function probe's `:in` and `:out`
//! share `n`).
//!
//! [`Replay::apply`] writes the recorded `:out` state onto the live Rust
//! graph (`crate::inject::apply_onto`): every table of the dump that exists in
//! the live graph at one of its dump paths (`env.player.output`,
//! `env.player.mainSkill.skillData`, …) is emptied and refilled in place, so
//! the caller's handles stay valid; tables the callee created are created.
//! The result is the state PoB had after the callee, as far as it is
//! reachable from `env` and the GlobalCache.

use std::collections::BTreeMap;

use crate::schema::StateDump;

/// Recorded callee deltas for one build run.
#[derive(Default)]
pub struct Replay {
    /// hit id of the callee's `:in` → its `:out` dump.
    pub deltas: BTreeMap<String, StateDump>,
}

impl Replay {
    /// Load the recorded `:in`/`:out` pairs of `callee` (a function probe id,
    /// e.g. `offence`, `runSkillFunc`, `skill:Barrage:preDamageFunc`) from a
    /// build run's dumps, keyed by hit id (`<callee>:in#<n>` or a file name).
    pub fn load(callee: &str, dumps: &BTreeMap<String, StateDump>) -> Replay {
        let probe_in = format!("{callee}:in");
        let probe_out = format!("{callee}:out");
        let mut deltas = BTreeMap::new();
        for d in dumps.values() {
            if d.probe != probe_in {
                continue;
            }
            let out = dumps
                .values()
                .find(|o| o.probe == probe_out && o.n == d.n && o.env_id == d.env_id);
            if let Some(out) = out {
                deltas.insert(d.hit.clone(), out.clone());
            }
        }
        Replay { deltas }
    }

    /// The `:out` dump recorded for `hit`.
    pub fn out_of(&self, hit: &str) -> Option<&StateDump> {
        self.deltas.get(hit)
    }

    /// Apply the delta recorded for `hit` to the live Rust state (returns false
    /// when the hit was not recorded or the state could not be applied).
    pub fn apply(&self, hit: &str, env: &pob_calc::env::EnvRef) -> bool {
        let Some(out) = self.deltas.get(hit) else {
            return false;
        };
        crate::inject::apply_onto(out, &env.build, env).is_ok()
    }
}
