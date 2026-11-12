//! pob-calc-core: the calc core (lua_table, cfg, classes, model, actor/env, the shared calc modules, probe export, the late seam)
//!
//! One of the six crates `pob-calc` is split into (CCR-0131, docs/port/calc-split-plan.md). The
//! files keep their paths under `src/` and their owners (calc doc §2.1); downstream code uses
//! `pob-calc`, which re-exports every public path.
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
pub mod cfg;
pub mod classes;
pub mod data_fns_types;
pub mod env;
pub mod flow;
pub mod global_cache;
pub mod late;
pub mod lua_table;
pub mod model;
pub mod module_state;
pub mod modules;
pub mod prelude;
pub mod probe;
pub mod stub;

pub use env::{EnvRef, Mode};
pub use flow::Flow;
pub use lua_table::{Key, LStr, Output, Table, Val};
pub use model::{Build, BuildRef};
