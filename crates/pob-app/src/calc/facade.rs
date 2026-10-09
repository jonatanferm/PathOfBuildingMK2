//! The single seam between UI and calc (ui doc §10.4, D14). Owner: U15 (frozen surface delivered
//! by UI-U0).
//!
//! lua: CalcsTab.lua:441-474 (BuildOutput), :760-763 (GetMiscCalculator), Calcs.lua:89-114
//! (getMiscCalculator), Build.lua:1255-1275 (where the recalc happens).

pub use crate::model::{BuildRef, EnvRef, MiscCalculator, Output, Override};

/// The single seam between UI and calc. Desktop: the Rust calc, synchronous (D14).
/// Every method may return Poll::Pending ONLY on wasm, where the calc runs in a Web Worker (calc doc D15).
/// Call sites handle Pending with the placeholder documented at each site; on desktop it never happens,
/// so parity is unaffected.
pub trait CalcFacade {
    /// calcsTab:BuildOutput() (CalcsTab.lua:441-474 via calc:PWR): fills build.calcsTab.{mainEnv,calcsEnv,mainOutput,
    /// calcsOutput}. Lua errors are caught as PoB does (calc doc D15 try_buildOutput) and reported via App::on_lua_error.
    fn build_output(&mut self, build: &BuildRef) -> std::task::Poll<()>;
    /// calcsTab:GetMiscCalculator() == calcs.getMiscCalculator(build) (Calcs.lua:89-114): (calcFunc, calcBase).
    fn misc_calculator(
        &mut self,
        build: &BuildRef,
    ) -> std::task::Poll<(std::rc::Rc<dyn MiscCalc>, Output)>;
}

/// `calcFunc(override, useFullDPS)`. Like the Lua closure it reads the LIVE build it was created for, so callers may
/// mutate the model temporarily and restore it (GemSelectControl:CalcOutputWithThisGem, ConfigTab tooltips).
pub trait MiscCalc {
    fn call(
        &self,
        ov: Option<std::rc::Rc<Override>>,
        use_full_dps: Option<bool>,
    ) -> std::task::Poll<Output>;
}
