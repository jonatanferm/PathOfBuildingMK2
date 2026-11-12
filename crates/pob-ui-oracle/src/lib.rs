//! `pob-ui-oracle` (DEV ONLY, never a dependency of a shipping binary; ui doc §11.2): unmodified
//! PoB Lua under LuaJIT drawing through OUR pob-sg, the determinism and order patches, the gallery
//! harness, the `ui-parity` CLI, `trace-render`, the `pob-lua-gui` reference viewer and the
//! `LuaCalcBridge`. Owner: U8 (`lua_calc`: U15). Module declarations: UI-U0.

pub mod boot;
pub mod diff_report;
pub mod gallery;
pub mod lua_calc;
pub mod markers;
pub mod oracle_cache;
pub mod patches;
pub mod runner;
pub mod sg_bridge;
