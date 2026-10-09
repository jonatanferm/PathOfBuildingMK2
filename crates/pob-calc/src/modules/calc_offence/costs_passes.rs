//! @pob-file Modules/CalcOffence.lua @16de4b82d
//! offence segment OF-C (CalcOffence.lua 1692-2173).
//!
//! Owner: OF-C. Stub created by U0: signatures are final (frozen; change through a CCR),
//! bodies are `unported!` until the owner ports them (calc doc D9, §10.2).
#![allow(
    non_snake_case,
    unused_variables,
    unused_imports,
    dead_code,
    clippy::all,
    clippy::pedantic
)]

use super::OffenceCtx;
use crate::prelude::*;

/// @pob Modules/CalcOffence.lua:1692-2173 h=b7eb396e
/// OF-C: CalcOffence.lua 1692-2173.
pub(crate) fn seg_costsAndPasses(ctx: &mut OffenceCtx) -> Flow {
    unported!("Modules/CalcOffence.lua:1692-2173")
}

/// `combineStat(stat, mode, ...)` (CalcOffence.lua:2066-2170), declared in OF-C, used after the
/// pass loops (§7.2). Its provenance is part of the OF-C segment marker above.
pub(crate) fn combineStat(ctx: &OffenceCtx, stat: Name, mode: Name, extra: Option<&Table>) {
    unported!("Modules/CalcOffence.lua:2066-2170")
}
