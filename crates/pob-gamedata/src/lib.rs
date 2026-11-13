//! Path of Building's game data for the Rust calc port (calc doc §2, §4.2-§4.4).
//!
//! * [`generated`]: the Global.lua enums and the `data.misc` records (P1's
//!   codegen, included from `crates/pob-dump/generated/`).
//! * [`modifier`] / [`value`]: `Mod`, `Tag`, `Value` (§4.3).
//! * [`schema`], [`data`], [`loader`]: the typed `data` (§4.4) and how it is
//!   loaded from the dump (`data.bin`, postcard; `pob-dump compile` writes it).
//! * [`sg_zlib`]: SimpleGraphic's `Inflate` / `Deflate` (FUP7: the one port).
//! * [`mod_tools`] (CT), [`skill_stat_map`] (CT), [`timeless_lut`] (LUT): unit
//!   stubs.
//!
//! No Lua runs here, and nothing depends on `mlua`. `Data` is immutable and
//! `Send + Sync`; its only mutable parts are the thread-local [`DataSel`].
#![allow(non_snake_case, non_upper_case_globals, non_camel_case_types)]
#![allow(
    clippy::type_complexity,
    clippy::struct_field_names,
    clippy::too_many_lines,
    clippy::wildcard_imports,
    clippy::ref_option,
    clippy::trivially_copy_pass_by_ref,
    clippy::large_types_passed_by_value,
    clippy::many_single_char_names,
    clippy::needless_lifetimes,
    clippy::elidable_lifetime_names,
    clippy::semicolon_if_nothing_returned,
    clippy::should_implement_trait
)]

pub mod data;
pub mod fnref;
pub mod generated;
pub mod loader;
pub mod mod_tools;
pub mod modifier;
pub mod schema;
pub mod sg_zlib;
pub mod skill_stat_map;
pub mod stub;
pub mod timeless_lut;
pub mod value;

pub use data::{
    data, init, init_with, is_initialised, jewelRadius, tree_bytes, Data, DataSel, ModCacheData,
    ModCacheEntry,
};
#[cfg(not(target_arch = "wasm32"))]
pub use data::{init_from_dir, load_dump_dir};
pub use fnref::LuaFnRef;
pub use generated::global::{
    colorCodes, ClearMatchKeywordFlagsCache, KeywordFlag, MatchKeywordFlags, ModFlag, SkillType,
    SkillTypeExpr, SkillTypeSet,
};
pub use loader::LoadError;
pub use modifier::{
    JewelFuncKind, JewelFuncSpec, Mod, ModRef, ModType, Ramp, Sockets, Tag, TagType,
};
pub use pob_lua::{n, nm, Name};
pub use schema::bases::{
    CostInfo, Costs, ItemBase, ItemBaseRef, UnarmedWeaponData, WeaponTypeInfo,
};
pub use schema::gems::{GemData, GemDataRef};
pub use schema::minions::{MinionData, MinionDataRef};
pub use schema::misc::{AilmentData, ClusterJewelData, JewelRadius};
pub use schema::skills::{
    GrantedEffect, GrantedEffectRef, LevelTable, SkillLevel, SkillLevelRef, SkillPart,
    StatMapEntry, StatMapItem,
};
pub use skill_stat_map::statMapLookup;
pub use value::{AStr, DataTable, DataValue, Value, ValueTable};

/// A set of names (`{ a = true, b = true }`), shared.
pub type NameSet = std::sync::Arc<ahash::AHashSet<Name>>;
