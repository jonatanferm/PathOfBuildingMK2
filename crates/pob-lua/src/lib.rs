//! Lua / LuaJIT 2.1 standard-library semantics for the Rust port of Path of
//! Building (calc doc §4.1). No Lua runs here: every function is a port of
//! the LuaJIT C source (vendored `luajit-src 210.5.12+a4f56a4`, the runtime
//! PoB's oracle runs) or of PoB's `Modules/Common.lua`, verified against real
//! LuaJIT by the L0 tests in `tests/` (dev-dependency on `pob-oracle`).
//!
//! * [`name`]: interned [`Name`] keys plus the `n!` / `nm!` / `cat!` macros.
//! * [`field`]: [`Field`] / [`Shared`] / [`PtrSet`] / [`PtrMap`], the aliasing
//!   model of calc doc D2 / §3.3.
//! * [`error`]: [`LuaError`] and `lua_error!` (D15).
//! * [`math`], [`bit`], [`format`], [`string`], [`pattern`], [`sort`],
//!   [`ipairs`], [`ljtab`]: unit P2.
//!
//! Numbers are `f64` everywhere; strings are byte strings with ASCII case
//! mapping (§3.5). Where a Lua byte operation can split a UTF-8 character the
//! byte-level function is the reference and the `&str` wrapper re-validates
//! lossily (ui doc §8 rule 4).
#![allow(non_snake_case, non_upper_case_globals, non_camel_case_types)]
// Faithful ports of LuaJIT C code keep its variable names and control flow
// (`goto init` → `continue`, `if (hi) hi--`, `lj_fls`), which these lints flag.
#![allow(
    clippy::many_single_char_names,
    clippy::needless_continue,
    clippy::manual_ilog2,
    clippy::manual_div_ceil,
    clippy::needless_range_loop,
    clippy::implicit_saturating_sub,
    clippy::field_reassign_with_default
)]

pub mod bit;
pub mod error;
pub mod field;
pub mod format;
pub mod ipairs;
pub mod ljtab;
pub mod math;
pub mod name;
pub mod pattern;
pub mod profile;
pub mod sort;
pub mod string;

// LuaJIT build profile (CCR-0001/0002, applied by CCR1): the shipped port
// always follows PoB's x64 single-number LuaJIT; see [`profile`].
pub use profile::{LJ_DUALNUM, LJ_TARGET_X86ORX64};

pub use error::LuaError;
pub use field::{CheapClone, Field, PtrMap, PtrSet, Shared};
/// The Lua-number → integer casts of LuaJIT on x64 (CCR-0033 §1).
pub use format::{cvtt_i32 as lj_num2int, cvtt_i64 as lua_tointeger};
pub use format::{lua_format, tonumber, tonumber_base, tostring_num, FmtArg, LuaConcat};
pub use ljtab::LjIntMap;
pub use name::{conditionName, multiplierName, names, CatBuf, Name};

// ---------------------------------------------------------------------------
// Short aliases used by the UI architecture (ui doc §8 table). They are plain
// re-exports of the functions above, so there is one implementation of each.
// ---------------------------------------------------------------------------
pub use format::tostring_num as tostring;
pub use math::{lmod as fmod_lua, round};
pub use pattern::{find, gmatch, gsub, gsub_fn, gsub_table, r#match as lmatch};
pub use sort::sort;
