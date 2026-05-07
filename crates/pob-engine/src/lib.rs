//! Path of Building calc engine — pure, Wasm-clean, no I/O.
//!
//! Rule of thumb: this crate must compile to `wasm32-unknown-unknown` without polyfills. That
//! means no `std::fs`, `std::env`, `std::process`, `std::net`, no `tokio`, no native windowing,
//! no `mlua`. `std::collections` and the rest of `core`/`alloc` are fine.
//!
//! See `docs/architecture-current.md` for the upstream Lua engine map.
