//! The calc facade (ui doc §10.4). Owner: U15 (module declarations: UI-U0).

pub mod facade;
pub mod rust;
#[cfg(target_arch = "wasm32")]
pub mod worker_web;

pub use facade::{CalcFacade, MiscCalc};
pub use rust::RustCalc;
