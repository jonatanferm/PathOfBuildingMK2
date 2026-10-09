//! LuaJIT `bit.*` (lib_bit.c, calc doc §4.1 `bit.rs`, §3.5 "Flags").
//!
//! LuaJIT converts every operand with `lj_num2bit` (modulo 2^32, rounding to
//! nearest-even) and returns a signed 32-bit result. Flag code works on `u32`
//! bit patterns; [`tobit`] converts a Lua number to that pattern and
//! [`tonum`] converts a result back to the Lua number LuaJIT returns.
//! Shift counts use the low 5 bits (`x86` `shl`/`shr`/`sar`).

/// `bit.band(a, b)`.
#[inline]
pub fn band(a: u32, b: u32) -> u32 {
    a & b
}

/// `bit.bor(a, b)`.
#[inline]
pub fn bor(a: u32, b: u32) -> u32 {
    a | b
}

/// `bit.bxor(a, b)`.
#[inline]
pub fn bxor(a: u32, b: u32) -> u32 {
    a ^ b
}

/// `bit.bnot(a)`.
#[inline]
pub fn bnot(a: u32) -> u32 {
    !a
}

/// `bit.lshift(a, n)`.
#[inline]
pub fn lshift(a: u32, n: u32) -> u32 {
    a << (n & 31)
}

/// `bit.rshift(a, n)` (logical).
#[inline]
pub fn rshift(a: u32, n: u32) -> u32 {
    a >> (n & 31)
}

/// `bit.arshift(a, n)` (arithmetic).
#[inline]
pub fn arshift(a: u32, n: u32) -> u32 {
    ((a as i32) >> (n & 31)) as u32
}

/// `bit.rol(a, n)`.
#[inline]
pub fn rol(a: u32, n: u32) -> u32 {
    a.rotate_left(n & 31)
}

/// `bit.ror(a, n)`.
#[inline]
pub fn ror(a: u32, n: u32) -> u32 {
    a.rotate_right(n & 31)
}

/// `bit.bswap(a)`.
#[inline]
pub fn bswap(a: u32) -> u32 {
    a.swap_bytes()
}

/// `bit.tobit(x)` (`lj_num2bit`: `(int32_t)lo32(x + 2^52 + 2^51)`).
/// LuaJIT only ever holds the canonical NaN (`0xfff8000000000000`, the C API
/// canonicalises every NaN it is handed), whose low word is 0, so any NaN
/// converts to 0.
#[inline]
pub fn tobit(x: f64) -> i32 {
    if x.is_nan() {
        return 0;
    }
    (x + 6_755_399_441_055_744.0).to_bits() as u32 as i32
}

/// A `u32` bit pattern as the (signed) Lua number LuaJIT's `bit.*` returns.
#[inline]
pub fn tonum(x: u32) -> f64 {
    f64::from(x as i32)
}

/// Lua number → `u32` bit pattern (`tobit` then reinterpret).
#[inline]
pub fn tou32(x: f64) -> u32 {
    tobit(x) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basics() {
        assert_eq!(tobit(4_294_967_295.0), -1);
        assert_eq!(tobit(2.5), 2);
        assert_eq!(tobit(3.5), 4);
        assert_eq!(tobit(-1.0), -1);
        assert_eq!(tonum(band(tou32(-1.0), 0xff)), 255.0);
        assert_eq!(tonum(bor(0x8000_0000, 1)), -2_147_483_647.0);
        assert_eq!(lshift(1, 33), 2);
        assert_eq!(rshift(0x8000_0000, 31), 1);
        assert_eq!(arshift(0x8000_0000, 31), 0xffff_ffff);
    }
}
