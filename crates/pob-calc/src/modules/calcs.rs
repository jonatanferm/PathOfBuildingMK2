//! @pob-file Modules/Calcs.lua @16de4b82d
//! Modules/Calcs.lua 1-872: buildOutput, calcFullDPS, getMiscCalculator, buildActiveSkill.
//! `infoDump` (38-88) is excluded (debug printing).
//!
//! Owner: CA. Stub created by U0: signatures are final (frozen; change through a CCR),
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

/// The closure `getMiscCalculator` returns (Calcs.lua:96-113): captures the build and `usedFullDPS`.
pub struct MiscCalculator {
    pub build: BuildRef,
    pub usedFullDPS: Cell<bool>,
}

impl MiscCalculator {
    /// `calcFunc(override, useFullDPS)`.
    pub fn call(&self, r#override: Rc<Override>, useFullDPS: Option<bool>) -> Output {
        unported!("Modules/Calcs.lua:96-113")
    }
}

/// `calcs.calcFullDPS` result (Calcs.lua:142-354).
#[derive(Default)]
pub struct FullDPS {
    pub combinedDPS: f64,
    pub TotalDotDPS: f64,
    pub skills: Table,
    pub TotalPoisonDPS: f64,
    pub causticGroundDPS: f64,
    pub impaleDPS: f64,
    pub igniteDPS: f64,
    pub burningGroundDPS: f64,
    pub bleedDPS: f64,
    pub corruptingBloodDPS: f64,
    pub decayDPS: f64,
    pub dotDPS: f64,
    pub cullingMulti: f64,
    pub extra: Table,
}

// @pob Modules/Calcs.lua:6-22 h=6c1fcf44
// (module-level statements: pairs,ipairs,t_insert,s_format,m_min,m_ceil,calcs,calcs.breakdownModule,CallStat,CallStat,CallStat,C…)

/// @pob Modules/Calcs.lua:25-35 h=a5dc713c
/// `math.average(t)` ("note this is unused").
pub fn average(t: &[f64]) -> f64 {
    unported!("Modules/Calcs.lua:25-35")
}

/// @pob Modules/Calcs.lua:89-114 h=14afc9e0
/// → (calculator, base output). The calculator calls `initEnv(build, "CALCULATOR", override)`
/// without a specEnv (Calcs.lua:89-114).
pub fn getMiscCalculator(build: &BuildRef) -> (Rc<MiscCalculator>, Output) {
    let () = unported!("Modules/Calcs.lua:89-114");
    (
        Rc::new(MiscCalculator {
            build: build.clone(),
            usedFullDPS: Cell::new(false),
        }),
        Table::new(),
    )
}

/// @pob Modules/Calcs.lua:116-140 h=197f3f34
pub(crate) fn getActiveSkillCount(activeSkill: &ActiveSkillRef) -> (f64, bool) {
    unported!("Modules/Calcs.lua:116-140")
}

/// @pob Modules/Calcs.lua:142-354 h=59802c76
pub fn calcFullDPS(
    build: &BuildRef,
    mode: Mode,
    r#override: Rc<Override>,
    specEnv: Option<&SpecEnv>,
) -> FullDPS {
    unported!("Modules/Calcs.lua:142-354")
}

/// @pob Modules/Calcs.lua:357-381 h=83413cac
pub fn buildActiveSkill(
    env: &EnvRef,
    mode: Mode,
    skill: &ActiveSkillRef,
    targetUUID: Option<Name>,
    limitedProcessingFlags: &[Name],
) {
    unported!("Modules/Calcs.lua:357-381")
}

/// @pob Modules/Calcs.lua:384-870 h=60c7a3ba
/// Calcs.lua:384. The stub runs the first statements (initEnv + perform) so the pipeline
/// runs end to end on stubs (U0-SKELETON; CA replaces the body).
pub fn buildOutput(build: &BuildRef, mode: Mode) -> EnvRef {
    let (env, _cachedPlayerDB, _cachedEnemyDB, _cachedMinionDB) =
        crate::modules::calc_setup::initEnv(build, mode, None, None);
    crate::modules::calc_perform::perform(&env, false);
    let () = unported!("Modules/Calcs.lua:384-870");
    env
}

// @pob Modules/Calcs.lua:872-872 h=ee9dedbd
// (module-level statements: Return)

// @pob-skip Modules/Calcs.lua:38-88 reason="infoDump: debug printing of an env (commented out at its only call site, Calcs.lua:867)"
