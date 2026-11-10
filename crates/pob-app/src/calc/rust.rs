//! `RustCalc`: the default `CalcFacade`, delegating to pob-calc (ui doc §10.4). Owner: U15.
//!
//! lua: CalcsTab.lua:441-474 (BuildOutput via calc:PWR), :760-762 (GetMiscCalculator via
//! calc:PWR), Calcs.lua:89-114 (getMiscCalculator and its `calcFunc` closure, calc:CA).
//! Always `Ready`. The ported bodies are calc:PWR's (`pob_calc::modules::power`) and calc:CA's
//! (`pob_calc::modules::calcs`); this file only adapts them to the facade, so it carries no
//! `@pob` markers (the lines are covered by those units).
//!
//! Lua errors (`lua_error!`, a panic with a `LuaError` payload, calc doc D15) are not caught
//! here: in PoB an error inside `calcsTab:BuildOutput()` aborts the rest of `Build:OnFrame` and is
//! caught by Launch's `PCall` around `main:OnFrame` (Launch.lua:112), which shows it with
//! `ShowErrMsg`. The frame driver that owns that `PCall` (U16, `App::on_lua_error`) catches it the
//! same way (`pob_lua::error::catch`), so the statements after the recalc are skipped as in PoB.

use std::rc::Rc;
use std::task::Poll;

use pob_calc::modules::calcs::MiscCalculator;
use pob_calc::modules::power;

use super::facade::{BuildRef, CalcFacade, MiscCalc, Output, Override};

#[derive(Default)]
pub struct RustCalc;

impl CalcFacade for RustCalc {
    /// `calcsTab:BuildOutput()` (CalcsTab.lua:441-472): `power::BuildOutput` fills
    /// `build.calcsTab.{mainEnv,mainOutput,calcsEnv,calcsOutput,miscCalculator}`.
    fn build_output(&mut self, build: &BuildRef) -> Poll<()> {
        power::BuildOutput(build);
        Poll::Ready(())
    }

    /// `calcsTab:GetMiscCalculator()` (CalcsTab.lua:760-762).
    fn misc_calculator(&mut self, build: &BuildRef) -> Poll<(Rc<dyn MiscCalc>, Output)> {
        let (calc_func, calc_base) = power::GetMiscCalculator(build);
        Poll::Ready((Rc::new(RustMiscCalc(calc_func)), calc_base))
    }
}

/// The `calcFunc` closure of `calcs.getMiscCalculator` (Calcs.lua:96-112).
pub struct RustMiscCalc(pub Rc<MiscCalculator>);

impl MiscCalc for RustMiscCalc {
    /// `calcFunc(override, useFullDPS)`. A nil override is the `{ }` initEnv makes of it
    /// (CalcSetup.lua:485 `local override = override or { }`); the callee writes into the table it is
    /// given (CalcSetup.lua:1424-1430, :1921-1922), so a caller's override is passed through by
    /// identity.
    fn call(&self, ov: Option<Rc<Override>>, use_full_dps: Option<bool>) -> Poll<Output> {
        let ov = ov.unwrap_or_else(|| Rc::new(Override::default()));
        Poll::Ready(self.0.call(ov, use_full_dps))
    }
}
