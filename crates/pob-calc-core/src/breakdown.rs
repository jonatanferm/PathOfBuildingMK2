//! `breakdown`: one `require(calcs.breakdownModule)(modDB, output, actor)`
//! instance — the data table plus the closure context (calc doc §5.5). The
//! helper functions (`breakdown.simple`, `.mod`, `.slot`, …) are CT's
//! (`modules/calc_breakdown.rs`).
#![allow(non_snake_case)]

use std::rc::Rc;

use pob_lua::CheapClone;

use crate::actor::{ActorRef, ActorWeak};
use crate::classes::ModStoreRef;
use crate::lua_table::{Output, Table};

#[derive(Clone, Default)]
pub struct Breakdown {
    /// The breakdown data table (`breakdown.Life = { … }`).
    pub t: Table,
    pub modDB: ModStoreRef,
    pub output: Output,
    pub actor: ActorWeak,
}

impl std::ops::Deref for Breakdown {
    type Target = Table;
    fn deref(&self) -> &Table {
        &self.t
    }
}

impl CheapClone for Breakdown {}

impl Breakdown {
    pub fn ptr_eq(&self, o: &Breakdown) -> bool {
        self.t.ptr_eq(&o.t)
    }
}

/// `LoadModule(calcs.breakdownModule, modDB, output, actor)` (CalcSetup /
/// CalcPerform create one per actor in MAIN/CALCS modes).
pub fn newBreakdown(modDB: &ModStoreRef, output: &Output, actor: &ActorRef) -> Breakdown {
    Breakdown {
        t: Table::new(),
        modDB: Rc::clone(modDB),
        output: output.clone(),
        actor: Rc::downgrade(actor),
    }
}
