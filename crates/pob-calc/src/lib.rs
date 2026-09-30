//! Faithful Rust port of Path of Building's calculation pipeline.
//!
//! Module layout follows PoB's Lua sources so each piece can be checked
//! against the file it came from; see `docs/parity-plan.md`.

pub mod flags;
pub mod headless;
#[cfg(all(not(target_arch = "wasm32"), feature = "luajit"))]
pub mod jit;
pub mod lua;
pub mod lua_pattern;
pub mod mod_parser;
pub mod mod_store;
pub mod modifier;
#[cfg(not(target_arch = "wasm32"))]
pub mod worker;

pub use mod_store::{ActorId, DbId, EvalEnv, ModCfg, ModDb, ModList, Store};
pub use modifier::{Mod, ModType, Tag, TagType, Value};
