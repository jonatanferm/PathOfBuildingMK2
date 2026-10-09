//! `LuaCalcBridge` (ui doc §10.4, D14; dev only, desktop): serialises the model to XML
//! (calc:BM-A), loads it into a second LuaJIT PoB (pob-oracle), runs the Lua calc, then injects
//! the resulting env/output/breakdown/modDB into the calc doc's Rust types through calc:ORC-X.
//! Owner: U15. UI-U0 stub.

use std::rc::Rc;
use std::task::Poll;

use pob_app::calc::facade::{BuildRef, CalcFacade, MiscCalc, Output};

#[derive(Default)]
pub struct LuaCalcBridge {}

impl CalcFacade for LuaCalcBridge {
    fn build_output(&mut self, build: &BuildRef) -> Poll<()> {
        let _ = build;
        todo!("U15: LuaCalcBridge::build_output")
    }
    fn misc_calculator(&mut self, build: &BuildRef) -> Poll<(Rc<dyn MiscCalc>, Output)> {
        let _ = build;
        todo!("U15: LuaCalcBridge::misc_calculator")
    }
}
