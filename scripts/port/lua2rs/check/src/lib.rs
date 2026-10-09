//! Compile check for lua2rs output (unit T1). Not part of the product.
//!
//! `gen/` is written by `python3 scripts/port/lua2rs check`; the other modules model the
//! calc-doc contract types the generated code is written against.
#![allow(non_snake_case, non_upper_case_globals, non_camel_case_types, dead_code, unused)]

#[path = "../../../../../crates/pob-dump/generated/global.rs"]
pub mod global;
#[path = "../../../../../crates/pob-dump/generated/misc.rs"]
pub mod misc;

pub mod calc;
pub mod gamedata;
pub mod modparser;
pub mod prelude;

pub mod gen;
