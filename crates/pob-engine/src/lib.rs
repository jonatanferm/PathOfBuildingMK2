//! Path of Building calc engine — pure, Wasm-clean, no I/O.
//!
//! Rule of thumb: this crate must compile to `wasm32-unknown-unknown` without polyfills. That
//! means no `std::fs`, `std::env`, `std::process`, `std::net`, no `tokio`, no native windowing,
//! no `mlua`. `std::collections` and the rest of `core`/`alloc` are fine.
//!
//! See `docs/architecture-current.md` for the upstream Lua engine map.
//!
//! ## Module map
//!
//! - [`modifier`] — `Mod`, `ModValue`, `ModType`, `Tag`, `Source`. The data carrier.
//! - [`mod_db`] — `ModStore`, `ModDB`, `ModList`. Storage + query API
//!   (`Sum` / `More` / `Flag` / `Override` / `List`).
//! - [`mod_parser`] — English text → `Vec<Mod>`. A tiny subset of PoB's `ModParser.lua`
//!   for Phase 2; expanded in Phase 3.
//! - [`character`] — `Character` (class + level + allocated nodes), `Build` (the user's
//!   configuration). Runtime version of PoB's `Build`.
//! - [`env`] — `Env`, `Output`. Computation context.
//! - [`perform`] — top-level `compute(build) -> Output`. Calls the basic-stats passes.

pub mod character;
pub mod env;
pub mod item_parser;
pub mod mod_db;
pub mod mod_parser;
pub mod modifier;
pub mod perform;
pub mod pob_export;
pub mod pob_import;
pub mod share;
pub mod skill;

pub use character::{Character, CharacterSnapshot, ClassRef, ConfigState};
pub use env::{Env, Output};
pub use item_parser::{apply_item_set, apply_item_set_with_bases, parse_item, ItemApplyReport};
pub use mod_db::{ModDB, ModList, ModStore};
pub use mod_parser::{parse_mod_line, ParsedMod};
pub use modifier::{Mod, ModType, ModValue, Source, Tag, TagKind};
pub use perform::{compute, compute_full};
pub use pob_export::{export_pob_code, export_pob_xml};
pub use pob_import::{import_pob_code, import_pob_xml, PobImportError};
pub use share::{export_code, import_code, ShareError};
pub use skill::{skill_mods, MainSkill, SkillRegistry};
