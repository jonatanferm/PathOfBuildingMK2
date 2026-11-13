//! LuaJIT build profile (CCR-0001, CCR-0002; orchestrator decision applied by CCR1).
//!
//! LuaJIT's observable number semantics depend on the architecture it is
//! built for. x86/x64 builds are single-number (every number is a double)
//! and convert floats to integers with `cvttsd2si` ("integer indefinite",
//! i.e. MIN, for NaN and out-of-range inputs). ARM64 builds are dual-number
//! (`LJ_DUALNUM`: integer-valued results are int32 values, which have no
//! `-0`) and convert with saturating `fcvtzs` (NaN -> 0).
//!
//! **The shipped port always follows the x64 profile**, PoB's own platform
//! (Windows x64), on every host and target (desktop aarch64 and wasm32
//! included). [`LJ_TARGET_X86ORX64`] and [`LJ_DUALNUM`] are therefore
//! constants.
//!
//! The ARM64 dual-number profile exists only so that the L0 tests can explain
//! the differences between the shipped profile and an ARM64 oracle LuaJIT:
//! with the `oracle-dualnum` feature (enabled only through pob-lua's own
//! dev-dependency on itself), [`with_luajit_arm64_profile`] runs a closure
//! with the ARM64 casts and dual-number `tonumber` zeros. Without the
//! feature [`x64_casts`] / [`dualnum`] are constant `true` / `false` and the
//! profile switch does not exist. (The two `hashrot` spellings in lj_tab.h
//! are the same function, so `LjIntMap` order is profile-independent.)

/// `LJ_TARGET_X86ORX64` of the LuaJIT build the port follows: always x64.
pub const LJ_TARGET_X86ORX64: bool = true;

/// `LJ_DUALNUM` of the LuaJIT build the port follows: always single-number.
pub const LJ_DUALNUM: bool = false;

#[cfg(feature = "oracle-dualnum")]
std::thread_local! {
    static ARM64: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Float->int casts follow x64 `cvttsd2si` (true in every shipped build).
#[inline(always)]
#[must_use]
pub fn x64_casts() -> bool {
    !arm64_active()
}

/// Dual-number scanning (`tonumber` zeros are int32 +0); false in every
/// shipped build.
#[inline(always)]
#[must_use]
pub fn dualnum() -> bool {
    arm64_active()
}

#[cfg(not(feature = "oracle-dualnum"))]
#[inline(always)]
fn arm64_active() -> bool {
    false
}

#[cfg(feature = "oracle-dualnum")]
#[inline]
fn arm64_active() -> bool {
    ARM64.with(std::cell::Cell::get)
}

/// ORACLE TESTS ONLY: run `f` with the ARM64 dual-number LuaJIT profile on
/// this thread (restored afterwards, also on unwind). L0 uses it to check that
/// every mismatch between the shipped x64 profile and an ARM64 oracle is one
/// of the documented build-profile differences (CCR-0001 table).
#[cfg(feature = "oracle-dualnum")]
pub fn with_luajit_arm64_profile<R>(f: impl FnOnce() -> R) -> R {
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            ARM64.with(|c| c.set(self.0));
        }
    }
    let _restore = Restore(ARM64.with(|c| c.replace(true)));
    f()
}

#[cfg(test)]
mod tests {
    #[test]
    fn shipped_profile_is_x64() {
        assert!(super::LJ_TARGET_X86ORX64);
        assert!(!super::LJ_DUALNUM);
        assert!(super::x64_casts());
        assert!(!super::dualnum());
        // x64 cvttsd2si: NaN / out of range -> MIN, whatever the host.
        assert_eq!(
            crate::lua_format("%d", &[crate::FmtArg::Num(1e30)]),
            "-9223372036854775808"
        );
        // lib_base.c tonumber(s, base ~= 10): "-0" is -0 single-number, int32 +0 dual-number.
        assert_eq!(
            crate::tonumber_base("-0", 2).map(|x| x.to_bits()),
            Some((-0.0f64).to_bits())
        );
    }

    #[cfg(feature = "oracle-dualnum")]
    #[test]
    fn arm64_profile_is_scoped() {
        let s = super::with_luajit_arm64_profile(|| {
            assert!(super::dualnum());
            crate::lua_format("%d", &[crate::FmtArg::Num(1e30)])
        });
        assert_eq!(s, "9223372036854775807");
        assert!(!super::dualnum());
        assert_eq!(
            super::with_luajit_arm64_profile(|| crate::tonumber_base("-0", 2)).map(|x| x.to_bits()),
            Some(0)
        );
    }
}
