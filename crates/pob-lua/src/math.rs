//! @pob-file Modules/Common.lua @16de4b82d
//! LuaJIT `math.*` and Lua arithmetic (`%`, `^`), plus PoB's rounding helpers
//! from `Modules/Common.lua` 709-800 and 1013-1023 (calc doc §4.1 `math.rs`).
//!
//! LuaJIT sources (vendored `luajit-src 210.5.12+a4f56a4`, x64):
//! * `math.floor`/`math.ceil`: `vm_x64.dasc` `vm_floor`/`vm_ceil` (exact,
//!   sign of zero kept), same as `f64::floor`/`f64::ceil`.
//! * `math.min`/`math.max`: `vm_x64.dasc` `math_minmax` = `minsd`/`maxsd`
//!   folded left to right, i.e. `a < b ? a : b` / `a > b ? a : b` (NaN and
//!   ±0 resolve to the *second* operand). `lj_vm_foldarith` (JIT constant
//!   folding) uses the same expressions.
//! * `%`: `vm_mod` = `a - floor(a/b)*b`.
//! * `^`: `BC_POW` calls libm `pow` (this LuaJIT has no `lj_vm_powi`); the JIT
//!   folds `x^0`→1, `x^1`→x, `x^2`→x*x. `x*x` is NOT always equal to
//!   `pow(x, 2)` (1 ulp apart for some `x`); the port follows the interpreter
//!   (CCR-0001), and [`lpow`] blocks LLVM from making the same fold.
//! * `math.log(x, b)`: `lib_math.c` = `log2(x) * (1/log2(b))`.
//! * `math.modf`: libm `modf`.
//!
//! libm functions (`exp`, `log`, `log2`, `pow`) come from the platform libm
//! like LuaJIT's; on wasm32 they come from Rust's bundled libm port.

/// `math.huge`.
pub const m_huge: f64 = f64::INFINITY;
/// `math.pi`.
pub const m_pi: f64 = std::f64::consts::PI;

/// `math.floor`.
#[inline]
pub fn m_floor(x: f64) -> f64 {
    x.floor()
}

/// `math.ceil`.
#[inline]
pub fn m_ceil(x: f64) -> f64 {
    x.ceil()
}

/// `math.abs`.
#[inline]
pub fn m_abs(x: f64) -> f64 {
    x.abs()
}

/// `math.sqrt`.
#[inline]
pub fn m_sqrt(x: f64) -> f64 {
    x.sqrt()
}

/// `math.exp`.
#[inline]
pub fn m_exp(x: f64) -> f64 {
    x.exp()
}

/// `math.log(x)` / `math.log(x, base)`.
#[inline]
pub fn m_log(x: f64, base: Option<f64>) -> f64 {
    match base {
        None => x.ln(),
        Some(y) => {
            let x = x.log2();
            let y = 1.0 / y.log2();
            x * y // Do NOT join the expression to x / y.
        }
    }
}

/// `math.log10`.
#[inline]
pub fn m_log10(x: f64) -> f64 {
    x.log10()
}

/// `math.modf(x)`: (integral part toward zero, fractional part), libm `modf`
/// (the fractional part carries the sign of `x`; `modf(±inf) = (±inf, ±0)`).
#[inline]
pub fn m_modf(x: f64) -> (f64, f64) {
    let i = x.trunc();
    let f = if x.is_infinite() { 0.0 } else { x - i };
    (i, f.copysign(x))
}

/// `math.min(a, b)` (`minsd`: `a < b ? a : b`).
#[inline]
pub fn m_min(a: f64, b: f64) -> f64 {
    if a < b {
        a
    } else {
        b
    }
}

/// `math.max(a, b)` (`maxsd`: `a > b ? a : b`).
#[inline]
pub fn m_max(a: f64, b: f64) -> f64 {
    if a > b {
        a
    } else {
        b
    }
}

/// `math.min(x1, x2, ...)`, folded left to right.
pub fn m_min_n(xs: &[f64]) -> f64 {
    let Some((&first, rest)) = xs.split_first() else {
        crate::lua_error!("bad argument #1 to 'min' (number expected, got no value)");
    };
    rest.iter().fold(first, |acc, &x| m_min(acc, x))
}

/// `math.max(x1, x2, ...)`, folded left to right.
pub fn m_max_n(xs: &[f64]) -> f64 {
    let Some((&first, rest)) = xs.split_first() else {
        crate::lua_error!("bad argument #1 to 'max' (number expected, got no value)");
    };
    rest.iter().fold(first, |acc, &x| m_max(acc, x))
}

/// Lua `a % b` (floored modulo, `vm_mod`): `a - floor(a/b)*b`.
#[inline]
pub fn lmod(a: f64, b: f64) -> f64 {
    a - (a / b).floor() * b
}

/// Lua `a ^ b` (and `math.pow`): libm `pow`, exactly as the interpreter's
/// `BC_POW` calls it (CCR-0001 names the interpreter as the reference).
///
/// Both operands go through [`std::hint::black_box`] so the result does not
/// depend on the optimisation level: once inlined with a constant operand,
/// LLVM's libcall simplifier rewrites `pow(x, 2.0)` to `x*x` (LuaJIT's JIT
/// fold, which differs from libm `pow` by 1 ulp for some `x`, e.g.
/// `-1.9948972224101745e7`), `pow(x, -1.0)` to `1.0/x`, `pow(2.0, n)` to
/// `exp2`/`ldexp` and `pow(10.0, x)` to `exp10`, and constant-folds `pow`
/// with two constant operands at compile time. `black_box` hides the
/// operands' values from those passes in every profile, so the call reaching
/// libm is always a plain `pow(a, b)`. Do not remove it.
#[inline]
pub fn lpow(a: f64, b: f64) -> f64 {
    std::hint::black_box(a).powf(std::hint::black_box(b))
}

/// `math.pow`.
#[inline]
pub fn m_pow(a: f64, b: f64) -> f64 {
    lpow(a, b)
}

/// Rounds a number to the nearest <dec> decimal places
/// @pob Modules/Common.lua:709-716 h=b2a501ce
pub fn round(val: f64, dec: Option<i32>) -> f64 {
    // L710
    if let Some(dec) = dec {
        // L711
        m_floor(val * lpow(10.0, f64::from(dec)) + 0.5) / lpow(10.0, f64::from(dec))
    } else {
        // L713
        m_floor(val + 0.5)
    }
}

/// Rounds down a number to the nearest <dec> decimal places
/// @pob Modules/Common.lua:717-729 h=e1c0a3d0
pub fn floor(val: f64, dec: Option<i32>) -> f64 {
    // L722
    if let Some(dec) = dec {
        // L723
        let mult = lpow(10.0, f64::from(dec));
        // L724
        m_floor(val * mult + 0.0001) / mult
    } else {
        // L726
        m_floor(val)
    }
}

/// Symmetric round with precision: Rounds towards zero to <dec> decimal places.
/// @pob Modules/Common.lua:730-749 h=4b00096e
pub fn roundSymmetric(val: f64, dec: Option<i32>) -> f64 {
    // L734
    if let Some(dec) = dec {
        // L735
        let factor = lpow(10.0, f64::from(dec));
        // L736
        if val >= 0.0 {
            m_floor(val * factor + 0.5) / factor
        } else {
            m_ceil(val * factor - 0.5) / factor
        }
    } else {
        // L742
        if val >= 0.0 {
            m_floor(val + 0.5)
        } else {
            m_ceil(val - 0.5)
        }
    }
}

/// Use rounding formula for positive numbers always used in corrupted unique roll ranges this is an incorrect way to round numbers.
/// @pob Modules/Common.lua:750-761 h=ef99f97f
pub fn alwaysPositiveRound(val: f64, dec: Option<i32>) -> f64 {
    // L754
    if let Some(dec) = dec {
        // L755
        let factor = lpow(10.0, f64::from(dec));
        // L756
        floorSymmetric(val * factor + 0.5, None) / factor
    } else {
        // L758
        floorSymmetric(val + 0.5, None)
    }
}

/// Symmetric floor with precision: Rounds down towards zero to <dec> decimal places.
/// @pob Modules/Common.lua:762-774 h=3641c52b
pub fn floorSymmetric(val: f64, dec: Option<i32>) -> f64 {
    // L767
    if let Some(dec) = dec {
        // L768
        let factor = lpow(10.0, f64::from(dec));
        // L769
        m_modf(val * factor).0 / factor
    } else {
        // L771
        m_modf(val).0
    }
}

/// Symmetric ceil with precision: Rounds up away from zero to <dec> decimal places.
/// @pob Modules/Common.lua:775-793 h=c60d06b4
pub fn ceilSymmetric(val: f64, dec: Option<i32>) -> f64 {
    // L779
    if let Some(dec) = dec {
        // L780
        let factor = lpow(10.0, f64::from(dec));
        // L781
        if val >= 0.0 {
            m_ceil(val * factor) / factor
        } else {
            m_floor(val * factor) / factor
        }
    } else {
        // L787
        if val >= 0.0 {
            m_ceil(val)
        } else {
            m_floor(val)
        }
    }
}

/// Returns the n-th triangular number
/// See https://en.wikipedia.org/wiki/Triangular_number
/// @pob Modules/Common.lua:794-800 h=1b25d022
pub fn triangular(n: f64) -> f64 {
    // L799
    n * (n + 1.0) / 2.0
}

/// Ceil function with optional base parameter (`base = base or 1`: pass 1.0 for nil)
/// @pob Modules/Common.lua:1013-1017 h=9d7b6f5d
pub fn ceil_b(x: f64, base: f64) -> f64 {
    // L1015
    base * m_ceil(x / base)
}

/// Ceil function with optional base parameter (`base = base or 1`: pass 1.0 for nil)
/// @pob Modules/Common.lua:1018-1023 h=a409cc32
pub fn floor_b(x: f64, base: f64) -> f64 {
    // L1021
    base * m_floor(x / base)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basics() {
        assert_eq!(m_min(f64::NAN, 1.0), 1.0);
        assert!(m_min(1.0, f64::NAN).is_nan());
        assert_eq!(m_min(0.0, -0.0).to_bits(), (-0.0f64).to_bits());
        assert_eq!(m_max(-0.0, 0.0).to_bits(), 0.0f64.to_bits());
        assert_eq!(lmod(-1.0, 3.0), 2.0);
        assert_eq!(lmod(5.5, -2.0), -0.5);
        assert_eq!(m_modf(-3.5), (-3.0, -0.5));
        assert_eq!(m_modf(f64::INFINITY), (f64::INFINITY, 0.0));
        assert_eq!(round(2.5, None), 3.0);
        assert_eq!(round(1.2345, Some(2)), 1.23);
        assert_eq!(floor(0.29, Some(2)), 0.29);
        assert_eq!(roundSymmetric(-2.5, None), -3.0);
        assert_eq!(floorSymmetric(-2.7, None), -2.0);
        assert_eq!(ceilSymmetric(-2.2, None), -3.0);
        assert_eq!(alwaysPositiveRound(-2.5, None), -2.0);
        assert_eq!(triangular(4.0), 10.0);
        assert_eq!(ceil_b(7.0, 5.0), 10.0);
        assert_eq!(floor_b(7.0, 5.0), 5.0);
        assert_eq!(m_min_n(&[3.0, 1.0, 2.0]), 1.0);
        assert_eq!(m_max_n(&[3.0, 1.0, 2.0]), 3.0);
        assert_eq!(m_log(8.0, Some(2.0)), 3.0);
    }

    /// `lpow` with a constant operand must reach libm `pow` in every profile
    /// (release used to fold `lpow(x, 2.0)` to `x*x`; run this test with
    /// `--release` too). The reference is `pow` called through an opaque
    /// function pointer, which no optimisation can see through.
    #[test]
    fn lpow_is_not_folded() {
        let pow: fn(f64, f64) -> f64 = std::hint::black_box(f64::powf);
        // x*x != pow(x, 2) for this x on glibc (LuaJIT `x^2` interpreted
        // gives 0x42f69f1a1699a1ee, a JIT trace gives x*x = ...a1ef).
        let x = -1.994_897_222_410_174_5e7;
        assert_eq!(lpow(x, 2.0).to_bits(), pow(x, 2.0).to_bits());
        for &x in &[
            x,
            0.1,
            1.1,
            3.7,
            -2.5e-3,
            123_456.789,
            1e300,
            f64::MIN_POSITIVE,
        ] {
            for &y in &[2.0, -1.0, 0.5, 3.0, -2.0, 0.0, 1.0] {
                assert_eq!(
                    lpow(x, y).to_bits(),
                    pow(x, y).to_bits(),
                    "lpow({x:e}, {y})"
                );
            }
        }
        for n in -8..=8 {
            let n = f64::from(n);
            assert_eq!(
                lpow(10.0, n).to_bits(),
                pow(10.0, n).to_bits(),
                "lpow(10, {n})"
            );
            assert_eq!(
                lpow(2.0, n).to_bits(),
                pow(2.0, n).to_bits(),
                "lpow(2, {n})"
            );
        }
    }
}
