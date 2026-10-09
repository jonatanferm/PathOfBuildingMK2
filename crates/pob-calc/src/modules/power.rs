//! @pob-file Classes/CalcsTab.lua @16de4b82d
//! CalcsTab.lua 441-773: BuildOutput, the power report (PowerBuilder as a resumable iterator),
//! CalculatePowerStat, CalculateCombinedOffDefStat. Sequential by default (D12).
//!
//! Owner: PWR. Stub created by U0: signatures are final (frozen; change through a CCR),
//! bodies are `unported!` until the owner ports them (calc doc D9, §10.2).
#![allow(
    non_snake_case,
    unused_variables,
    unused_imports,
    dead_code,
    clippy::all,
    clippy::pedantic
)]

use crate::prelude::*;

/// Power report options: `parallel` builds one Build per worker from the XML text (opt-in, D12).
#[derive(Default, Clone, Copy)]
pub struct PowerOptions {
    pub parallel: bool,
}

/// The PowerBuilder coroutine's state between resumes.
#[derive(Default)]
pub struct PowerState {
    pub done: bool,
    pub extra: Table,
}

/// @pob Classes/CalcsTab.lua:441-472 h=eb7f2e5e
/// `CalcsTab:BuildOutput()`.
pub fn BuildOutput(build: &BuildRef) {
    unported!("Classes/CalcsTab.lua:441-472")
}

/// @pob Classes/CalcsTab.lua:475-493 h=5c678572
/// `CalcsTab:BuildPower()`: starts the power report.
pub fn BuildPower(build: &BuildRef) {
    unported!("Classes/CalcsTab.lua:475-493")
}

/// @pob Classes/CalcsTab.lua:496-740 h=04c2908a
/// `CalcsTab:PowerBuilder()` (a coroutine in Lua): one resumable step per call; `None` when done.
pub fn PowerBuilder(
    state: &mut PowerState,
    build: &BuildRef,
    options: &PowerOptions,
) -> Option<()> {
    unported!("Classes/CalcsTab.lua:496-740")
}

/// @pob Classes/CalcsTab.lua:742-746 h=8e17c3b0
pub fn CalculatePowerStat(selection: &Table, original: &Output, modified: &Output) -> f64 {
    unported!("Classes/CalcsTab.lua:742-746")
}

/// @pob Modules/Data.lua:186-212 h=0cefea12
/// `data.powerStatList.GetFromOutput(output, statTable, skipTransform)` (called by
/// `CalculatePowerStat`; `statTable` is a `data.powerStatList` entry or a `{ stat = … }` table).
pub fn GetFromOutput(output: &Output, statTable: &Table, skipTransform: bool) -> f64 {
    unported!("Modules/Data.lua:186-212")
}

/// @pob Modules/Data.lua:131-131 h=107327e2
/// @pob Modules/Data.lua:164-168 h=e900d657
/// The `transform` closures of `data.powerStatList` entries (`PowerStat::transform`), by
/// closure id: `Name` strips a leading "The "; the five `*TakenHit` stats negate.
pub fn powerStatTransform(fnId: &str, value: Val) -> Val {
    unported!("Modules/Data.lua:131,164-168")
}

/// @pob Classes/CalcsTab.lua:748-758 h=d5dfc379
pub fn CalculateCombinedOffDefStat(original: &Output, modified: &Output) -> f64 {
    unported!("Classes/CalcsTab.lua:748-758")
}

/// @pob Classes/CalcsTab.lua:760-762 h=49183536
pub fn GetMiscCalculator(build: &BuildRef) -> (Rc<crate::modules::calcs::MiscCalculator>, Output) {
    let () = unported!("Classes/CalcsTab.lua:760-762");
    crate::modules::calcs::getMiscCalculator(build)
}

/// @pob Classes/CalcsTab.lua:764-766 h=ced0e4c0
/// UI undo state (no calc effect).
pub fn CreateUndoState(build: &BuildRef) -> Table {
    unported!("Classes/CalcsTab.lua:764-766")
}

/// @pob Classes/CalcsTab.lua:768-773 h=f936be82
/// UI undo state (no calc effect).
pub fn RestoreUndoState(build: &BuildRef, state: &Table) {
    unported!("Classes/CalcsTab.lua:768-773")
}
