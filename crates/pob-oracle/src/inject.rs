//! Injection of a pob-state/1 dump into the Rust calc graph (calc doc §9.2,
//! "Inject and export").
//!
//! Owner: ORC-X. Stub created by U0 with the final signatures: `state`
//! rebuilds the Rust graph (env, actors, stores, skills, cfgs) with its
//! sharing intact and returns the dumped locals by name, which
//! `pob_calc::probe::ProbeState` maps onto a segment ctx.

use std::collections::BTreeMap;

use pob_calc::env::EnvRef;
use pob_calc::lua_table::Val;
use pob_calc::model::BuildRef;

use crate::schema::StateDump;

/// The dumped locals of a probe hit, by Lua local name.
pub type Locals = BTreeMap<String, Val>;

/// Rebuild the Rust graph of a probe dump on top of `build` (which supplies the
/// build model the dump refers to). `None` until ORC-X ports it.
pub fn state(dump: &StateDump, build: &BuildRef) -> Option<(EnvRef, Locals)> {
    let _ = (dump, build);
    pob_calc::unported!("inject::state (ORC-X)")
}
