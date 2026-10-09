//! Lua runtime errors (ui doc §6.14, D17). Owner: U4.
//!
//! A Lua runtime error (nil index, nil call, assert, error()) panics with a `pob_lua::LuaError`
//! payload. The host catches it per callback and shows PoB's error prompt (Launch.lua ShowErrMsg).
//! One error type across UI and calc: the calc plan's macro.

/// the calc plan's macro (LuaError payload); one error type across UI and calc
pub use pob_lua::lua_error;
pub use pob_lua::LuaError;
