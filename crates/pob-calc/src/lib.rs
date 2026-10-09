//! Faithful Rust port of Path of Building's calculation pipeline.
//!
//! Module layout follows PoB's Lua sources so each piece can be checked
//! against the file it came from; see `docs/parity-plan.md`. No Lua runs
//! here: PoB itself (under LuaJIT) lives in the `pob-oracle` crate, which is
//! the test oracle for this one.

pub mod flags;
pub mod lua_pattern;
pub mod mod_store;
pub mod modifier;
pub mod modifier_serde;

pub use mod_store::{ActorId, DbId, EvalEnv, ModCfg, ModDb, ModList, Store};
pub use modifier::{Mod, ModType, Tag, TagType, Value};
