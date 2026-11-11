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
//! Layout: the code lives in six crates (CCR-0131, docs/port/calc-split-plan.md), and this
//! crate re-exports every public path, so `pob_calc::…` is unchanged for downstream code:
//! * `pob-calc-core`: [`lua_table`], [`cfg`], [`classes`], [`actor`], [`active_skill`], [`env`],
//!   [`breakdown`], [`global_cache`], [`module_state`], [`flow`], [`stub`], [`model`], [`probe`]
//!   and the shared modules ([`modules`]`::{calc_tools, common, calc_format, calc_breakdown,
//!   mod_tools_env, jewel_funcs, calc_base, calc_perform_shared}`);
//! * `pob-calc-setup`: `modules::{calc_setup, calc_active_skill}`;
//! * `pob-calc-config`: `modules::{config_options, mod_map}`;
//! * `pob-calc-defence`: `modules::calc_defence`;
//! * `pob-calc-offence`: `modules::calc_offence`, [`data_fns`] (skill hooks);
//! * `pob-calc-perform`: `modules::{calc_perform, calc_triggers, calc_mirages, calcs, power}`,
//!   [`calcs_facade`], `probe::run_segment`, and the provider of the core's late-bound calls.
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

pub use pob_calc_core::{
    active_skill, actor, breakdown, cfg, classes, env, flow, global_cache, lua_table, model,
    module_state, prelude, stub,
};
pub use pob_calc_core::{ctx_struct, seg, tbl, unported};
pub use pob_calc_offence::data_fns;
pub use pob_calc_perform::{calcs_facade, calcs_facade::calcs};

#[cfg(not(target_arch = "wasm32"))]
pub use pob_calc_perform::calcs_facade::try_buildOutput;

/// State export for the oracle (`pob-calc-core`'s `probe`), plus the stage dispatcher
/// [`run_segment`](probe::run_segment) (`pob-calc-perform`, feature `probe`).
pub mod probe {
    pub use pob_calc_core::probe::*;
    #[cfg(feature = "probe")]
    pub use pob_calc_perform::probe_segments::run_segment;
}

/// `Modules/*` (calc doc §2.1), from the crate each module lives in.
pub mod modules {
    pub use pob_calc_config::modules::{config_options, mod_map};
    pub use pob_calc_core::modules::*;
    pub use pob_calc_defence::modules::calc_defence;
    pub use pob_calc_offence::modules::calc_offence;
    pub use pob_calc_perform::modules::{calc_mirages, calc_perform, calc_triggers, calcs, power};
    pub use pob_calc_setup::modules::{calc_active_skill, calc_setup};
}

pub use env::{EnvRef, Mode};
pub use flow::Flow;
pub use lua_table::{Key, LStr, Output, Table, Val};
pub use model::{Build, BuildRef};
