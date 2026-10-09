//! Faithful Rust port of Path of Building's `Modules/ModParser.lua` (calc doc §6):
//! mod lines to `Mod`s. Jewel closures are emitted as data
//! (`Value::JewelFunc(JewelFuncSpec)`); their bodies run in pob-calc.
//!
//! Layout (owners: calc doc §2.1): `types` (U0), `helpers`/`scan`/`mod_cache`/
//! `generated_lists` (MP0), `form_list`/`mod_name_list` (MP1),
//! `mod_flag_list`/`pre_flag_list`/`mod_tag_list` (MP2), `special_a`..`d`
//! (MP3-MP6), `jewel_func_list` (MP7).
#![allow(non_snake_case, non_upper_case_globals, non_camel_case_types)]
#![allow(clippy::type_complexity)]

// `unported!` / `tag!` in every module (the lua2rs-generated lists call them unqualified).
#[allow(unused_imports)]
#[macro_use]
extern crate pob_gamedata;

pub mod form_list;
pub mod generated_lists;
pub mod helpers;
pub mod jewel_func_list;
pub mod mod_cache;
pub mod mod_flag_list;
pub mod mod_name_list;
pub mod mod_tag_list;
pub mod pre_flag_list;
pub mod scan;
pub mod special_a;
pub mod special_b;
pub mod special_c;
pub mod special_d;
pub mod types;

pub use mod_cache::{parseMod, parseModNative};
pub use pob_gamedata::unported;
pub use scan::scan;
pub use types::{Cap, Caps, ModSpec, PatternEntry, PatternList, SpecialVal, TagVal};
