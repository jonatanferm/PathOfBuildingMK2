//! PoB's `Modules/Calc*.lua` and helpers, one Rust module per Lua module (or
//! per split giant function). Owners per file: calc doc §2.1.
//!
//! This crate (pob-calc-core) holds the modules the other calc crates share; the others are
//! declared by the crate that holds them (CCR-0131) and re-exported by `pob_calc::modules`.

pub mod calc_base;
pub mod calc_breakdown;
pub mod calc_format;
pub mod calc_perform_shared;
pub mod calc_tools;
pub mod common;
pub mod jewel_funcs;
pub mod mod_tools_env;
