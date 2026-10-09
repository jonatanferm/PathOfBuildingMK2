//! Lua runtime errors (calc doc D15, §3.2 `error(...)` row).
//!
//! `lua_error!` panics with a [`LuaError`] payload, so a Lua `error()` (or an
//! arithmetic / index / call on nil) unwinds exactly as far as Lua's would: to
//! the nearest [`catch`] (PoB's `pcall` around the calc). On
//! `wasm32-unknown-unknown` (`panic=abort`) there is no unwinding; the calc
//! runs in a Web Worker that the UI restarts after an abort.

/// The payload of a Lua runtime error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LuaError(pub String);

impl std::fmt::Display for LuaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for LuaError {}

/// Raise a Lua runtime error: `lua_error!("bad thing {}", x)`.
#[macro_export]
macro_rules! lua_error {
    ($($t:tt)*) => { ::std::panic::panic_any($crate::error::LuaError(format!($($t)*))) }
}

/// Run `f`, turning a `lua_error!` raised inside it into `Err` (Lua `pcall`).
/// Any other panic is not a Lua error and keeps unwinding.
/// Desktop only (wasm32-unknown-unknown is panic=abort; see D15).
#[cfg(not(target_arch = "wasm32"))]
pub fn catch<R>(f: impl FnOnce() -> R) -> Result<R, LuaError> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(r) => Ok(r),
        Err(payload) => match payload.downcast::<LuaError>() {
            Ok(e) => Err(*e),
            Err(other) => std::panic::resume_unwind(other),
        },
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn catch_lua_error() {
        let r: Result<i32, _> = catch(|| crate::lua_error!("attempt to index a nil value ({})", 3));
        assert_eq!(r, Err(LuaError("attempt to index a nil value (3)".into())));
        assert_eq!(catch(|| 5), Ok(5));
    }

    #[test]
    #[should_panic(expected = "not lua")]
    fn other_panics_propagate() {
        let _ = catch(|| panic!("not lua"));
    }
}
