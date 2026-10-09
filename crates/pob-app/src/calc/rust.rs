//! `RustCalc`: the default `CalcFacade`, delegating to pob-calc (ui doc §10.4). Owner: U15.
//! UI-U0 stub.
//!
//! lua: CalcsTab.lua:441-474 (BuildOutput via calc:PWR), Calcs.lua:89-114 (getMiscCalculator).
//! Always `Ready`.

use std::rc::Rc;
use std::task::Poll;

use super::facade::{BuildRef, CalcFacade, MiscCalc, Output};

#[derive(Default)]
pub struct RustCalc;

impl CalcFacade for RustCalc {
    fn build_output(&mut self, build: &BuildRef) -> Poll<()> {
        let _ = build;
        todo!("U15: RustCalc::build_output (calcsTab:BuildOutput via calc:PWR)")
    }
    fn misc_calculator(&mut self, build: &BuildRef) -> Poll<(Rc<dyn MiscCalc>, Output)> {
        let _ = build;
        todo!("U15: RustCalc::misc_calculator (calcs.getMiscCalculator)")
    }
}
