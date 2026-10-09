//! Path of Building itself, run headless.
//!
//! Everything Lua lives here, split out of `pob-calc` so the Rust port of
//! PoB's calculations stays free of any Lua runtime:
//!
//! * [`jit`] boots PoB under LuaJIT (desktop), [`headless`] under the small
//!   pure-Rust interpreter in [`lua`] (wasm / fallback); [`worker`] runs one
//!   on a background thread for the app's UI.
//! * [`mod_parser`] runs PoB's `ModParser.lua` unchanged.
//! * [`oracle`] is the parity harness: per-build dumps of PoB's complete
//!   calc output and of its intermediate stages (mod databases, active
//!   skills), produced by the `pob_oracle` binary in a fresh process per
//!   build, and typed accessors for tests. See `README.md`.

pub mod headless;
#[cfg(all(not(target_arch = "wasm32"), feature = "luajit"))]
pub mod jit;
pub mod lua;
pub mod mod_parser;
pub mod oracle;
#[cfg(not(target_arch = "wasm32"))]
pub mod worker;

pub use oracle::{Oracle, OracleOutput, OracleStages};
