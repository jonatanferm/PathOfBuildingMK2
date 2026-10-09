//! Faithful Rust port of Path of Building's calculation pipeline (calc doc
//! §2, §5-§8): from a build model to `env.player.output`, the breakdowns, the
//! minion and enemy outputs and the stores the Calcs tab tabulates.
//!
//! The port is a literal 1:1 mirror of PoB's Lua (D1): identifiers, function
//! decomposition, statement order and comments are kept, so every function
//! can be diffed against the Lua it comes from (its `@pob` marker names the
//! lines). Aliased mutable Lua tables are `Rc` handles with interior
//! mutability (D2); computed-key tables are dynamic [`lua_table::Table`]s
//! (D3). No Lua runs here (D4); `pob-oracle` is the test oracle.
//!
//! Layout (owners per file: calc doc §2.1):
//! * core types (U0): [`lua_table`], [`cfg`], [`classes`] (types), [`actor`],
//!   [`active_skill`], [`env`], [`breakdown`], [`global_cache`],
//!   [`module_state`], [`flow`], [`stub`], [`model`] (types), [`calcs_facade`];
//! * the ported modules: [`modules`] (Calc*.lua, CalcsTab power, config
//!   options, mod map, jewel funcs), [`classes`] (ModStore/ModDB/ModList,
//!   Item, PassiveTree, PassiveSpec), [`model`] (build/tab loaders),
//!   [`data_fns`] (skill hooks).
//!
//! Single-threaded by design (`Rc`, `!Send`), like PoB's one Lua VM (D12).
#![allow(non_snake_case, non_upper_case_globals, non_camel_case_types)]
#![allow(
    clippy::type_complexity,
    clippy::too_many_arguments,
    clippy::new_ret_no_self,
    clippy::wrong_self_convention,
    clippy::should_implement_trait,
    clippy::module_inception,
    clippy::struct_field_names,
    clippy::ref_option,
    clippy::trivially_copy_pass_by_ref,
    clippy::fn_params_excessive_bools,
    clippy::unused_self,
    clippy::used_underscore_binding,
    clippy::wildcard_imports,
    clippy::many_single_char_names,
    clippy::semicolon_if_nothing_returned,
    clippy::single_match_else
)]

pub mod active_skill;
pub mod actor;
pub mod breakdown;
pub mod calcs_facade;
pub mod cfg;
pub mod classes;
pub mod data_fns;
pub mod env;
pub mod flow;
pub mod global_cache;
pub mod lua_table;
pub mod model;
pub mod module_state;
pub mod modules;
pub mod prelude;
pub mod probe;
pub mod stub;

pub use calcs_facade::calcs;
#[cfg(not(target_arch = "wasm32"))]
pub use calcs_facade::try_buildOutput;
pub use env::{EnvRef, Mode};
pub use flow::Flow;
pub use lua_table::{Key, LStr, Output, Table, Val};
pub use model::{Build, BuildRef};
