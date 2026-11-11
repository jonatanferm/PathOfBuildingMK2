//! @pob-file Modules/CalcPerform.lua @16de4b82d
//! The parts of CalcPerform.lua that code below the perform crate uses (CCR-0131;
//! docs/port/calc-split-plan.md B2, B8), moved here whole from their owner files:
//!
//! * `calcs.actionSpeedMod` (CalcPerform.lua 1141-1151; owner PF-B, from
//!   `calc_perform/actor_b.rs`, which re-exports it): defence and offence call it;
//! * the closure-upvalue structs [`FlaskUpvalues`] and [`TinctureUpvalues`] (owner PF-C, from
//!   `calc_perform/setup_flasks.rs`, which re-exports them): the probe reflection reads them.
//!
//! The bodies are unchanged; only their file changed.
#![allow(non_snake_case, clippy::all, clippy::pedantic)]

use crate::prelude::*;

/// @pob Modules/CalcPerform.lua:1141-1151 h=c94cebd4
pub fn actionSpeedMod(actor: &ActorRef) -> f64 {
    // L1142
    let modDB = actor.modDB.get();
    // L1143
    let minimumActionSpeed = modDB.Max(None, nm!["MinimumActionSpeed"]).unwrap_or(0.0);
    // L1144
    let maximumActionSpeedReduction = modDB.Max(None, nm!["MaximumActionSpeedReduction"]);
    // L1145
    let mut actionSpeedMod = 1.0
        + (m_max(
            -data().misc.TemporalChainsEffectCap,
            modDB.Sum(ModType::INC, None, nm!["TemporalChainsActionSpeed"]),
        ) + modDB.Sum(ModType::INC, None, nm!["ActionSpeed"]))
            / 100.0;
    // L1146
    actionSpeedMod = m_max(minimumActionSpeed / 100.0, actionSpeedMod);
    // L1147
    if let Some(maximumActionSpeedReduction) = maximumActionSpeedReduction {
        // L1148
        actionSpeedMod = m_min(
            (100.0 - maximumActionSpeedReduction) / 100.0,
            actionSpeedMod,
        );
    }
    // L1150
    actionSpeedMod
}

// ---- Rust-only plumbing ----------------------------------------------------------------------

crate::ctx_struct! {
    /// The upvalues of the local function `mergeFlasks` (CalcPerform.lua 1691; with its local
    /// functions `getFlaskInstantRecovery` 1638, `calcFlaskRecovery` 1652, `calcFlaskMods` 1701):
    /// the segment locals declared at 1602-1611, captured when the closure is created. The
    /// `PerformCtx.mergeFlasks` field holds them (CCR-0079 §1); the line probe `perform:1691`
    /// dumps them under these names (`env`, `modDB` and `nonUniqueFlasksApplyToMinion` are ctx
    /// fields).
    pub struct FlaskUpvalues {
        effectInc: f64,
        effectIncMagic: f64,
        effectIncMagicNoAdjacent: f64,
        effectIncNonPlayer: f64,
        effectIncMagicNonPlayer: f64,
        flasksApplyToMinion: bool,
        quickSilverAppliesToAllies: bool,
        flaskTotalRateInc: f64,
        flaskDurInc: f64,
    }
}

crate::ctx_struct! {
    /// The upvalues of the local function `mergeTinctures` (CalcPerform.lua 1862; with
    /// `calcTinctureMods` 1872): the tincture `effectInc`/`effectIncMagic` (1851-1852, shadowing
    /// the flask ones) and `tinctureLimit` (1853), which `mergeTinctures` decrements (1909), so it
    /// is shared by every call of the closure. `PerformCtx.mergeTinctures` holds them (CCR-0079
    /// §1); the line probe `perform:1862` dumps them.
    pub struct TinctureUpvalues {
        effectInc: f64,
        effectIncMagic: f64,
        tinctureLimit: Cell<f64>,
    }
}
