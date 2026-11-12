//! The calc facade (ui doc §10.4). Owner: U15 (module declarations: UI-U0).

pub mod facade;
pub mod rust;
// CCR-0134 §1: compiled on every target (the client state machine and the worker's request
// handler are target-independent and tested on desktop); its Web Worker glue is wasm32-only.
pub mod worker_web;

pub use facade::{CalcFacade, MiscCalc};
pub use rust::RustCalc;
