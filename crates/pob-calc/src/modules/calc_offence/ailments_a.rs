//! @pob-file Modules/CalcOffence.lua @16de4b82d
//! offence ailment prelude and ailment loop body A, OF-G (CalcOffence.lua 4155-4989). The local
//! functions `checkWeapon1HFlags` (4308), `calcAverageSourceDamage` (4315) and `calcAilmentDamage`
//! (4333) cross into OF-H, so they take the ctx and the loop locals (§7.2).
//!
//! Owner: OF-G. Stub created by U0: signatures are final (frozen; change through a CCR),
//! bodies are `unported!` until the owner ports them (calc doc D9, §10.2).
#![allow(
    non_snake_case,
    unused_variables,
    unused_imports,
    dead_code,
    clippy::all,
    clippy::pedantic
)]

use super::{AilmentPassLocals, OffenceCtx};
use crate::prelude::*;

/// @pob Modules/CalcOffence.lua:4155-4163 h=64e58855
/// OF-G: CalcOffence.lua 4155-4163 (ailment prelude).
pub(crate) fn seg_ailmentPrelude(ctx: &mut OffenceCtx) -> Flow {
    unported!("Modules/CalcOffence.lua:4155-4163")
}

/// @pob Modules/CalcOffence.lua:4165-4989 h=cb5977f2
/// OF-G: CalcOffence.lua 4165-4989 (sets `globalOutput` at 4165).
pub(crate) fn seg_bleedPoison(ctx: &mut OffenceCtx, al: &mut AilmentPassLocals) -> Flow {
    unported!("Modules/CalcOffence.lua:4165-4989")
}

// Loop-local functions declared in OF-G (provenance: the OF-G segment marker above).
pub(crate) fn checkWeapon1HFlags(ctx: &OffenceCtx, al: &AilmentPassLocals, cfg: &Cfg) -> bool {
    unported!("Modules/CalcOffence.lua:4308-4314")
}

/// → (min, max).
pub(crate) fn calcAverageSourceDamage(
    ctx: &OffenceCtx,
    al: &AilmentPassLocals,
    ailment: Name,
    sourceType: Name,
) -> (f64, f64) {
    unported!("Modules/CalcOffence.lua:4315-4332")
}

pub(crate) fn calcAilmentDamage(
    ctx: &OffenceCtx,
    al: &AilmentPassLocals,
    ailment: Name,
    sourceHitDmg: f64,
    sourceCritDmg: f64,
    extra: Option<&Table>,
) -> Table {
    unported!("Modules/CalcOffence.lua:4333-4400")
}
