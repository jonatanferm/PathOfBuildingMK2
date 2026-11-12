//! The provider side of the late-bound calls (`crate::late`, CCR-0131;
//! docs/port/calc-split-plan.md §3.4).
//!
//! Owner: CSPLIT. Expands the core's `late_fns!` list into one
//! `#[export_name = "pob_calc_late_<name>"]` function per entry, each calling the real body
//! (named in [`imp`]). The provider bodies are type-checked against the real functions; the
//! core's extern declarations match them because the list's types are full paths.

/// The real bodies, by the names the `late_fns!` list uses.
mod imp {
    pub(super) use crate::modules::calc_perform::{doActorLifeMana, doActorLifeManaReservation};
    pub(super) use crate::modules::calcs::misc_calculator_call;
    pub(super) use crate::modules::config_options::configApply;
    pub(super) use crate::modules::power::BuildOutput;
}

/// The definition side: one exported symbol per entry, forwarding to [`imp`].
macro_rules! late_provide {
    ($($name:ident($($a:ident : $t:ty),* $(,)?) -> $r:ty;)*) => {
        /// The seam's definition side (CCR-0131): `#[export_name]` is `unsafe_code`.
        #[allow(unsafe_code, non_snake_case, clippy::unused_unit)]
        mod seam {
            $(
                #[export_name = concat!("pob_calc_late_", stringify!($name))]
                pub fn $name($($a: $t),*) -> $r {
                    super::imp::$name($($a),*)
                }
            )*
        }
    };
}

crate::late_fns!(late_provide);
