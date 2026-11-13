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
//! * The probe oracle (calc-architecture.md §9.2): [`patcher`] copies and
//!   patches PoB's sources with line / function probes and write
//!   provenance, [`runner`] runs the patched PoB (one build per process,
//!   machine-wide lock, probe cache), [`schema`] reads the `pob-state/1`,
//!   hit-index and provenance files, [`diff`] compares PoB against the
//!   Rust calc, [`nondet`] records PoB's run-to-run differences.

#[cfg(not(target_arch = "wasm32"))]
pub mod diff;
pub mod headless;
/// ORC-X: injection of pob-state/1 dumps into the Rust graph (stub by U0).
#[cfg(not(target_arch = "wasm32"))]
pub mod inject;
#[cfg(all(not(target_arch = "wasm32"), feature = "luajit"))]
pub mod jit;
pub mod lua;
/// ORC-X: mods, tags and mod values from the oracle's JSON into `pob_gamedata` types.
pub mod mod_json;
pub mod mod_parser;
#[cfg(not(target_arch = "wasm32"))]
pub mod nondet;
pub mod oracle;
/// TP1: bounded parallel corpus loops for the oracle tests (re-exported by `testkit`). GOLD1
/// moved the pool into pob-golden (GOLDEN-PLAN §1.7); this path stays for the live tests.
#[cfg(not(target_arch = "wasm32"))]
pub use pob_golden::par;
#[cfg(not(target_arch = "wasm32"))]
pub mod patcher;
/// ORC-X: replay of recorded callee deltas (stub by U0).
#[cfg(not(target_arch = "wasm32"))]
pub mod replay;
#[cfg(not(target_arch = "wasm32"))]
pub mod runner;
#[cfg(not(target_arch = "wasm32"))]
pub mod schema;
/// ORC-X: helpers for the unit acceptance tests (stub by U0).
#[cfg(not(target_arch = "wasm32"))]
pub mod testkit;
#[cfg(not(target_arch = "wasm32"))]
pub mod worker;

pub use oracle::{Oracle, OracleOutput, OracleStages};

/// The LuaJIT binding (mlua with the vendored LuaJIT PoB's oracle runs), for
/// L0 tests that compare `pob-lua` against real LuaJIT (calc unit P2).
#[cfg(all(not(target_arch = "wasm32"), feature = "luajit"))]
pub use mlua;
