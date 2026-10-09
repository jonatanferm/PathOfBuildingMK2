//! @pob-file Modules/CalcPerform.lua @16de4b82d
//! perform segment PF-C (CalcPerform.lua 1289-1942): flasks, tinctures, minion setup, keystones.
//!
//! Owner: PF-C. Stub created by U0: signatures are final (frozen; change through a CCR),
//! bodies are `unported!` until the owner ports them (calc doc D9, §10.2).
#![allow(
    non_snake_case,
    unused_variables,
    unused_imports,
    dead_code,
    clippy::all,
    clippy::pedantic
)]

use super::PerformCtx;
use crate::prelude::*;

/// @pob Modules/CalcPerform.lua:1289-1942 h=f67a66ce
/// PF-C: CalcPerform.lua 1289-1942 (flasks, tinctures, minion setup, keystones).
pub(crate) fn seg(ctx: &mut PerformCtx) -> Flow {
    unported!("Modules/CalcPerform.lua:1289-1942")
}

// The local functions `mergeFlasks` (CalcPerform.lua:1691) and `mergeTinctures` (1862) are
// declared inside the segment and cross into PF-F (3121) / PF-D (2095) (§7.1); their
// provenance is part of the segment marker above.
pub(super) fn mergeFlasks(
    ctx: &PerformCtx,
    flasks: &PtrMap<Item, bool>,
    onlyRecovery: bool,
    checkNonRecoveryFlasksForMinions: bool,
) {
    unported!("Modules/CalcPerform.lua:1691-1861")
}

pub(super) fn mergeTinctures(ctx: &PerformCtx, tinctures: &PtrMap<Item, bool>) {
    unported!("Modules/CalcPerform.lua:1862-1942")
}
