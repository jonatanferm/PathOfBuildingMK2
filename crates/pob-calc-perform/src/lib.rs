//! pob-calc-perform: CalcPerform, CalcTriggers, CalcMirages, Calcs, the power report, the `calcs` facade, the probe's stage dispatcher and the late-seam provider.
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

// Mirror root (calc-split-plan §3.5): the core's crate root, re-exported, so the moved files keep
// every `crate::…` path (`crate::prelude::*`, `crate::modules::calc_tools`, `crate::ctx_struct!`).
pub use pob_calc_core::{
    active_skill, actor, breakdown, cfg, classes, data_fns_types, env, flow, global_cache, late,
    lua_table, model, module_state, prelude, probe, stub,
};
pub use pob_calc_core::{ctx_struct, late_fns, seg, tbl, unported};
pub use pob_calc_core::{Build, BuildRef, EnvRef, Flow, Key, LStr, Mode, Output, Table, Val};
pub use pob_calc_offence::data_fns;

pub mod calcs_facade;
pub use calcs_facade::calcs;
#[cfg(not(target_arch = "wasm32"))]
pub use calcs_facade::try_buildOutput;

mod late_provider;
#[cfg(feature = "probe")]
pub mod probe_segments;

pub mod modules {
    pub use pob_calc_config::modules::{config_options, mod_map};
    pub use pob_calc_core::modules::*;
    pub use pob_calc_defence::modules::calc_defence;
    pub use pob_calc_offence::modules::calc_offence;
    pub use pob_calc_setup::modules::{calc_active_skill, calc_setup};
    pub mod calc_mirages;
    pub mod calc_perform;
    pub mod calc_triggers;
    pub mod calcs;
    pub mod power;
}
