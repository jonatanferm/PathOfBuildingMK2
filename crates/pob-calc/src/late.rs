//! Late-bound calls from the calc core to the calc crates above it (CCR-0131;
//! docs/port/calc-split-plan.md §3.4).
//!
//! Owner: CSPLIT. Five calls run from core code into code that lives in a higher calc crate:
//!
//! | call | caller (core) | provider (pob-calc-perform's `late_provider.rs`) |
//! |---|---|---|
//! | `BuildOutput(build)` | `model/build_xml.rs` (Build.lua:695) | `modules::power::BuildOutput` |
//! | `configApply(r)` | `model/config_build_mod_list.rs` (ConfigTab.lua:1139) | `modules::config_options::configApply` |
//! | `misc_calculator_call(this, …)` | [`MiscCalculator::call`] | `modules::calcs::misc_calculator_call` |
//! | `doActorLifeMana(actor)` | `calc_defence/primary.rs` (CalcDefence.lua) | `modules::calc_perform::doActorLifeMana` |
//! | `doActorLifeManaReservation(actor, addAura)` | `calc_defence/primary.rs` | `modules::calc_perform::doActorLifeManaReservation` |
//!
//! The mechanism is a link-time symbol seam: [`late_fns!`](crate::late_fns) is the one list of
//! the five signatures. This module expands it into an `extern "Rust"` declaration of each
//! symbol `pob_calc_late_<name>` plus a safe `pub fn <name>` that calls it; the provider crate
//! expands the same list into `#[export_name = "pob_calc_late_<name>"]` functions that call the
//! real bodies. There is no registration call and no runtime cost (a direct call); a binary
//! that links the core without the provider fails to link (`undefined symbol:
//! pob_calc_late_…`), it never fails at run time.
//!
//! **Safety rule** (CCR-0131): the extern declaration is not type-checked against the provider,
//! so both sides must name the same types. Every type in the `late_fns!` list is therefore a
//! full path: `$crate::…` for core types (the `$crate` token keeps naming the core when the
//! list is expanded in the provider crate), `::std::…`/`::core::…` for std types (bool as
//! `::core::primitive::bool`) and `::<crate>::…` for other crates. Each provider body calls the
//! real function, so the provider side is checked against the real signature as well.
//! `allow(unsafe_code)` is limited to the two macro-generated `seam` modules (this one and the
//! provider's).
#![allow(non_snake_case)]

use crate::prelude::*;

/// The closure `getMiscCalculator` returns (Calcs.lua:100-113): captures the build and
/// `usedFullDPS`. Defined here (in the core) because `CalcsTab.miscCalculator` holds it; the body
/// of the closure is `modules::calcs::misc_calculator_call`, reached through the seam.
pub struct MiscCalculator {
    pub build: BuildRef,
    pub usedFullDPS: Cell<bool>,
}

impl MiscCalculator {
    /// `calcFunc(override, useFullDPS)` (Calcs.lua:100-113; marker on `getMiscCalculator`).
    pub fn call(&self, r#override: Rc<Override>, useFullDPS: Option<bool>) -> Output {
        misc_calculator_call(self, r#override, useFullDPS)
    }
}

/// `apply(val, modList, enemyModList, build)` (ConfigOptions.lua).
pub type ConfigApply = fn(val: &Val, modList: &ModList, enemyModList: &ModList, build: &BuildRef);

/// The late-bound signatures, expanded by `$cb!` (this module's `late_decl!`, the provider's
/// `late_provide!`). Every type is a full path (see the module doc: the safety rule).
#[macro_export]
#[doc(hidden)]
macro_rules! late_fns {
    ($cb:ident) => {
        $cb! {
            BuildOutput(build: &$crate::model::BuildRef) -> ();
            configApply(r: &::pob_gamedata::LuaFnRef) -> $crate::late::ConfigApply;
            misc_calculator_call(
                this: &$crate::late::MiscCalculator,
                r#override: ::std::rc::Rc<$crate::env::Override>,
                useFullDPS: ::core::option::Option<::core::primitive::bool>
            ) -> $crate::lua_table::Output;
            doActorLifeMana(actor: &$crate::actor::ActorRef) -> ();
            doActorLifeManaReservation(
                actor: &$crate::actor::ActorRef,
                addAura: ::core::primitive::bool
            ) -> ();
        }
    };
}

/// The declaration side: one `extern "Rust"` symbol and one safe wrapper per entry.
macro_rules! late_decl {
    ($($name:ident($($a:ident : $t:ty),* $(,)?) -> $r:ty;)*) => {
        /// The seam's declaration side (CCR-0131): the only `unsafe` of the calc core.
        #[allow(unsafe_code, clippy::unused_unit)]
        mod seam {
            mod ext {
                extern "Rust" {
                    $(
                        #[link_name = concat!("pob_calc_late_", stringify!($name))]
                        pub(super) fn $name($($a: $t),*) -> $r;
                    )*
                }
            }
            $(
                pub fn $name($($a: $t),*) -> $r {
                    // SAFETY: the symbol is defined by the provider's `late_provide!` expansion
                    // of the same `late_fns!` list, whose types are full paths (module doc), so
                    // the declaration and the definition have the same Rust signature and ABI.
                    unsafe { ext::$name($($a),*) }
                }
            )*
        }
        pub use seam::{$($name),*};
    };
}

crate::late_fns!(late_decl);
