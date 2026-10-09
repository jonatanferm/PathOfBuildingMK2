//! Module-level mutable upvalues that persist across calls (D17), thread-local
//! like the Lua VM (calc doc §5.6, §7.3).
//!
//! * `globalOutput` / `globalBreakdown` (CalcOffence.lua:65/67): assigned at
//!   2176/2607/4165, read by `combineStat` (2137) and the explosiveArrowFunc
//!   hook (3038).
//! * CalcOffence.lua:27-29, CalcSetup.lua:16 and CalcDefence.lua:22
//!   `tempTableN` are scratch tables that do not escape (verified:
//!   buildConversionTable 1937-1983, CalcSetup 1465, CalcDefence 862), so the
//!   ports use locals.
//! * CalcActiveSkill.lua:21 `mergeLevelCache` is AS's thread-local memo keyed
//!   by (Arc pointer of the data mod, value bits).
#![allow(non_snake_case)]

use pob_lua::Field;

use crate::breakdown::Breakdown;
use crate::lua_table::Output;

#[derive(Default)]
pub struct OffenceModState {
    /// CalcOffence.lua:65.
    pub globalOutput: Field<Option<Output>>,
    /// CalcOffence.lua:67.
    pub globalBreakdown: Field<Option<Breakdown>>,
}

thread_local! {
    pub static OFFENCE_STATE: OffenceModState = OffenceModState::default();
}
