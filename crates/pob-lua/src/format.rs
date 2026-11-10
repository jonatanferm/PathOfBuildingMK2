//! `string.format`, `tostring(number)`, `tonumber` and `..` with LuaJIT 2.1
//! semantics (calc doc §3.5, §4.1 `format.rs`).
//!
//! Ports of the vendored LuaJIT (`luajit-src 210.5.12+a4f56a4`, GC64), x64
//! single-number build profile (PoB's platform) on every host; the ARM64
//! dual-number branches exist only for oracle tests ([`crate::profile`],
//! CCR-0001/0002):
//! * `lj_strfmt.c`: `lj_strfmt_parse`, `lj_strfmt_putfxint`,
//!   `lj_strfmt_putfnum_int/uint`, `strfmt_putquotedlen`,
//!   `lj_strfmt_putfchar`, `strfmt_putfstrlen`, `lj_strfmt_putarg`.
//! * `lj_strfmt_num.c`: `lj_strfmt_wfnum` and its "nd" big-decimal helpers
//!   (exact, correctly rounded `%a %e %f %g`), used with `STRFMT_G14` for
//!   `tostring(number)`.
//! * `lj_strscan.c`: `lj_strscan_scan` with `STRSCAN_OPT_TONUM` (the
//!   `tonumber(s)` path), and `lib_base.c` `tonumber(s, base)`.
//!
//! The C code works on NUL-terminated buffers; [`at`] returns 0 past the end
//! so every `*p` read behaves the same, including on embedded NULs.
//!
//! LuaJIT spellings: `inf`, `-inf`, `nan` (LuaJIT 2.1's own formatter never
//! prints a sign for NaN), `-0`.

use crate::name::CatBuf;

// ---------------------------------------------------------------------------
// Arguments
// ---------------------------------------------------------------------------

/// One `string.format` argument.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FmtArg<'a> {
    Num(f64),
    Str(&'a str),
    Bool(bool),
    Nil,
}

impl From<f64> for FmtArg<'_> {
    fn from(v: f64) -> Self {
        FmtArg::Num(v)
    }
}
impl From<i64> for FmtArg<'_> {
    fn from(v: i64) -> Self {
        FmtArg::Num(v as f64)
    }
}
impl From<i32> for FmtArg<'_> {
    fn from(v: i32) -> Self {
        FmtArg::Num(f64::from(v))
    }
}
impl From<u32> for FmtArg<'_> {
    fn from(v: u32) -> Self {
        FmtArg::Num(f64::from(v))
    }
}
impl From<usize> for FmtArg<'_> {
    fn from(v: usize) -> Self {
        FmtArg::Num(v as f64)
    }
}
impl From<bool> for FmtArg<'_> {
    fn from(v: bool) -> Self {
        FmtArg::Bool(v)
    }
}
impl<'a> From<&'a str> for FmtArg<'a> {
    fn from(v: &'a str) -> Self {
        FmtArg::Str(v)
    }
}
impl<'a> From<&'a String> for FmtArg<'a> {
    fn from(v: &'a String) -> Self {
        FmtArg::Str(v.as_str())
    }
}
impl From<crate::Name> for FmtArg<'static> {
    fn from(v: crate::Name) -> Self {
        FmtArg::Str(v.as_str())
    }
}
impl<'a> From<&'a crate::Name> for FmtArg<'a> {
    fn from(v: &'a crate::Name) -> Self {
        FmtArg::Str(v.as_str())
    }
}
/// `LStr` (`Rc<str>`, calc doc §5.1).
impl<'a> From<&'a std::rc::Rc<str>> for FmtArg<'a> {
    fn from(v: &'a std::rc::Rc<str>) -> Self {
        FmtArg::Str(v)
    }
}
impl From<Option<f64>> for FmtArg<'_> {
    fn from(v: Option<f64>) -> Self {
        v.map_or(FmtArg::Nil, FmtArg::Num)
    }
}
impl<'a> From<Option<&'a str>> for FmtArg<'a> {
    fn from(v: Option<&'a str>) -> Self {
        v.map_or(FmtArg::Nil, FmtArg::Str)
    }
}
impl From<()> for FmtArg<'_> {
    fn from((): ()) -> Self {
        FmtArg::Nil
    }
}

/// `s_format(fmt, ...)` → `lfmt!(fmt, ...)`.
#[macro_export]
macro_rules! lfmt { ($f:expr $(, $a:expr)*) => { $crate::format::lua_format($f, &[$($crate::format::FmtArg::from($a)),*]) } }

/// Alias used by the UI architecture doc (ui doc §8).
#[macro_export]
macro_rules! lformat { ($f:expr $(, $a:expr)*) => { $crate::lfmt!($f $(, $a)*) } }

// ---------------------------------------------------------------------------
// lj_strfmt.h
// ---------------------------------------------------------------------------

type SFormat = u32;

const STRFMT_EOF: u32 = 0;
const STRFMT_ERR: u32 = 1;
const STRFMT_LIT: u32 = 2;
const STRFMT_INT: u32 = 3;
const STRFMT_UINT: u32 = 4;
const STRFMT_NUM: u32 = 5;
const STRFMT_STR: u32 = 6;
const STRFMT_CHAR: u32 = 7;
const STRFMT_PTR: u32 = 8;

const STRFMT_T_HEX: u32 = 0x0010;
const STRFMT_T_OCT: u32 = 0x0020;
const STRFMT_T_FP_A: u32 = 0x0000;
const STRFMT_T_FP_E: u32 = 0x0010;
const STRFMT_T_FP_F: u32 = 0x0020;
const STRFMT_T_FP_G: u32 = 0x0030;
const STRFMT_T_QUOTED: u32 = 0x0010;

const STRFMT_F_LEFT: u32 = 0x0100;
const STRFMT_F_PLUS: u32 = 0x0200;
const STRFMT_F_ZERO: u32 = 0x0400;
const STRFMT_F_SPACE: u32 = 0x0800;
const STRFMT_F_ALT: u32 = 0x1000;
const STRFMT_F_UPPER: u32 = 0x2000;

const STRFMT_SH_WIDTH: u32 = 16;
const STRFMT_SH_PREC: u32 = 24;

#[inline]
fn strfmt_type(sf: SFormat) -> u32 {
    sf & 15
}
#[inline]
fn strfmt_width(sf: SFormat) -> u32 {
    (sf >> STRFMT_SH_WIDTH) & 255
}
#[inline]
fn strfmt_prec(sf: SFormat) -> u32 {
    ((sf >> STRFMT_SH_PREC) & 255).wrapping_sub(1)
}
#[inline]
fn strfmt_fp(sf: SFormat) -> u32 {
    (sf >> 4) & 3
}

const STRFMT_A: u32 = STRFMT_NUM | STRFMT_T_FP_A;
const STRFMT_C: u32 = STRFMT_CHAR;
const STRFMT_D: u32 = STRFMT_INT;
const STRFMT_E: u32 = STRFMT_NUM | STRFMT_T_FP_E;
const STRFMT_F: u32 = STRFMT_NUM | STRFMT_T_FP_F;
const STRFMT_G: u32 = STRFMT_NUM | STRFMT_T_FP_G;
const STRFMT_I: u32 = STRFMT_D;
const STRFMT_O: u32 = STRFMT_UINT | STRFMT_T_OCT;
const STRFMT_P: u32 = STRFMT_PTR;
const STRFMT_Q: u32 = STRFMT_STR | STRFMT_T_QUOTED;
const STRFMT_S: u32 = STRFMT_STR;
const STRFMT_U: u32 = STRFMT_UINT;
const STRFMT_X: u32 = STRFMT_UINT | STRFMT_T_HEX;
const STRFMT_G14: u32 = STRFMT_G | ((14 + 1) << STRFMT_SH_PREC);

// ---------------------------------------------------------------------------
// lj_char.c (ASCII table; 128-255 are identifier chars only)
// ---------------------------------------------------------------------------

pub(crate) const LJ_CHAR_CNTRL: u8 = 0x01;
pub(crate) const LJ_CHAR_SPACE: u8 = 0x02;
pub(crate) const LJ_CHAR_PUNCT: u8 = 0x04;
pub(crate) const LJ_CHAR_DIGIT: u8 = 0x08;
pub(crate) const LJ_CHAR_XDIGIT: u8 = 0x10;
pub(crate) const LJ_CHAR_UPPER: u8 = 0x20;
pub(crate) const LJ_CHAR_LOWER: u8 = 0x40;
pub(crate) const LJ_CHAR_ALPHA: u8 = LJ_CHAR_LOWER | LJ_CHAR_UPPER;
pub(crate) const LJ_CHAR_ALNUM: u8 = LJ_CHAR_ALPHA | LJ_CHAR_DIGIT;
pub(crate) const LJ_CHAR_GRAPH: u8 = LJ_CHAR_ALNUM | LJ_CHAR_PUNCT;

/// `lj_char_bits + 1`, i.e. indexed by the byte value.
#[rustfmt::skip]
static LJ_CHAR_BITS: [u8; 256] = [
    1,  1,  1,  1,  1,  1,  1,  1,  1,  3,  3,  3,  3,  3,  1,  1,
    1,  1,  1,  1,  1,  1,  1,  1,  1,  1,  1,  1,  1,  1,  1,  1,
    2,  4,  4,  4,  4,  4,  4,  4,  4,  4,  4,  4,  4,  4,  4,  4,
  152,152,152,152,152,152,152,152,152,152,  4,  4,  4,  4,  4,  4,
    4,176,176,176,176,176,176,160,160,160,160,160,160,160,160,160,
  160,160,160,160,160,160,160,160,160,160,160,  4,  4,  4,  4,132,
    4,208,208,208,208,208,208,192,192,192,192,192,192,192,192,192,
  192,192,192,192,192,192,192,192,192,192,192,  4,  4,  4,  4,  1,
  128,128,128,128,128,128,128,128,128,128,128,128,128,128,128,128,
  128,128,128,128,128,128,128,128,128,128,128,128,128,128,128,128,
  128,128,128,128,128,128,128,128,128,128,128,128,128,128,128,128,
  128,128,128,128,128,128,128,128,128,128,128,128,128,128,128,128,
  128,128,128,128,128,128,128,128,128,128,128,128,128,128,128,128,
  128,128,128,128,128,128,128,128,128,128,128,128,128,128,128,128,
  128,128,128,128,128,128,128,128,128,128,128,128,128,128,128,128,
  128,128,128,128,128,128,128,128,128,128,128,128,128,128,128,128,
];

/// `lj_char_isa(c, t)`.
#[inline]
pub(crate) fn lj_char_isa(c: u8, t: u8) -> bool {
    LJ_CHAR_BITS[c as usize] & t != 0
}
#[inline]
pub(crate) fn lj_char_isspace(c: u8) -> bool {
    lj_char_isa(c, LJ_CHAR_SPACE)
}
#[inline]
pub(crate) fn lj_char_isdigit(c: u8) -> bool {
    lj_char_isa(c, LJ_CHAR_DIGIT)
}

/// `*p` on a NUL-terminated C buffer.
#[inline]
pub(crate) fn at(s: &[u8], i: usize) -> u8 {
    if i < s.len() {
        s[i]
    } else {
        0
    }
}

// ---------------------------------------------------------------------------
// C float→int casts of the LuaJIT build profile (CCR-0001, [`crate::profile`]):
// x86/x64 `cvttsd2si` (the shipped profile) gives the "integer indefinite"
// value (MIN) for NaN and out-of-range inputs; ARM64 `fcvtzs` (oracle tests
// only) saturates and maps NaN to 0 (which is Rust's `as`).
// ---------------------------------------------------------------------------

/// `(int64_t)n`: `(lua_Integer)n`, i.e. `lua_tointeger` on 64-bit LuaJIT (what a C module such
/// as `lua-utf8` sees through `luaL_checkinteger`). Public for the UI (CCR-0033 §1).
#[inline]
pub fn cvtt_i64(n: f64) -> i64 {
    if crate::profile::x64_casts()
        && (n.is_nan() || n >= 9_223_372_036_854_775_808.0 || n < -9_223_372_036_854_775_808.0)
    {
        i64::MIN
    } else {
        n as i64
    }
}

/// `(int32_t)n` (`lj_num2int`): the conversion of a number argument by `lj_lib_checkint` /
/// `lj_lib_optint` (the `string.sub` fast path's `cvttsd2si r32`). Public for the UI (CCR-0033 §1).
#[inline]
pub fn cvtt_i32(n: f64) -> i32 {
    if crate::profile::x64_casts() && (n.is_nan() || n >= 2_147_483_648.0 || n <= -2_147_483_649.0) {
        i32::MIN
    } else {
        n as i32
    }
}

// ---------------------------------------------------------------------------
// Format parser (lj_strfmt_parse)
// ---------------------------------------------------------------------------

struct FormatState<'a> {
    fmt: &'a [u8],
    p: usize,
    e: usize,
    str: usize,
    len: usize,
}

#[rustfmt::skip]
static STRFMT_MAP: [u32; (b'x' - b'A') as usize + 1] = [
  STRFMT_A,0,0,0,STRFMT_E,STRFMT_F,STRFMT_G,0,0,0,0,0,0,
  0,0,0,0,0,0,0,0,0,0,STRFMT_X,0,0,
  0,0,0,0,0,0,
  STRFMT_A,0,STRFMT_C,STRFMT_D,STRFMT_E,STRFMT_F,STRFMT_G,0,STRFMT_I,0,0,0,0,
  0,STRFMT_O,STRFMT_P,STRFMT_Q,0,STRFMT_S,0,STRFMT_U,0,0,STRFMT_X
];

fn lj_strfmt_parse(fs: &mut FormatState<'_>) -> SFormat {
    let f = fs.fmt;
    let mut p = fs.p;
    let e = fs.e;
    fs.str = p;
    while p < e {
        if f[p] == b'%' {
            // Escape char?
            if at(f, p + 1) == b'%' {
                // '%%'?
                p += 1;
                fs.p = p + 1;
                fs.len = p - fs.str;
                return if fs.len != 0 { STRFMT_LIT } else { STRFMT_EOF };
            }
            let mut sf: SFormat = 0;
            if p != fs.str {
                break;
            }
            p += 1;
            while u32::from(at(f, p)).wrapping_sub(u32::from(b' ')) <= u32::from(b'0' - b' ') {
                // Parse flags.
                match at(f, p) {
                    b'-' => sf |= STRFMT_F_LEFT,
                    b'+' => sf |= STRFMT_F_PLUS,
                    b'0' => sf |= STRFMT_F_ZERO,
                    b' ' => sf |= STRFMT_F_SPACE,
                    b'#' => sf |= STRFMT_F_ALT,
                    _ => break,
                }
                p += 1;
            }
            if u32::from(at(f, p)).wrapping_sub(u32::from(b'0')) < 10 {
                // Parse width.
                let mut width = u32::from(at(f, p) - b'0');
                p += 1;
                if u32::from(at(f, p)).wrapping_sub(u32::from(b'0')) < 10 {
                    width = u32::from(at(f, p) - b'0') + width * 10;
                    p += 1;
                }
                sf |= width << STRFMT_SH_WIDTH;
            }
            if at(f, p) == b'.' {
                // Parse precision.
                let mut prec = 0u32;
                p += 1;
                if u32::from(at(f, p)).wrapping_sub(u32::from(b'0')) < 10 {
                    prec = u32::from(at(f, p) - b'0');
                    p += 1;
                    if u32::from(at(f, p)).wrapping_sub(u32::from(b'0')) < 10 {
                        prec = u32::from(at(f, p) - b'0') + prec * 10;
                        p += 1;
                    }
                }
                sf |= (prec + 1) << STRFMT_SH_PREC;
            }
            // Parse conversion.
            let c = u32::from(at(f, p)).wrapping_sub(u32::from(b'A'));
            if c <= u32::from(b'x' - b'A') {
                let sx = STRFMT_MAP[c as usize];
                if sx != 0 {
                    fs.p = p + 1;
                    return sf | sx | (if c & 0x20 != 0 { 0 } else { STRFMT_F_UPPER });
                }
            }
            // Return error location.
            if at(f, p) >= 32 {
                p += 1;
            }
            fs.len = p - fs.str;
            fs.p = fs.e;
            return STRFMT_ERR;
        }
        p += 1;
    }
    fs.p = p;
    fs.len = p - fs.str;
    if fs.len != 0 {
        STRFMT_LIT
    } else {
        STRFMT_EOF
    }
}

// ---------------------------------------------------------------------------
// Raw conversions
// ---------------------------------------------------------------------------

/// `lj_strfmt_wint`: decimal `int32_t`.
fn wint(out: &mut [u8], mut p: usize, k: i32) -> usize {
    let mut tmp = itoa_buf();
    let s = fmt_i64(&mut tmp, i64::from(k));
    out[p..p + s.len()].copy_from_slice(s);
    p += s.len();
    p
}

fn itoa_buf() -> [u8; 24] {
    [0u8; 24]
}

fn fmt_i64(buf: &mut [u8; 24], k: i64) -> &[u8] {
    let neg = k < 0;
    let mut u = k.unsigned_abs();
    let mut q = buf.len();
    loop {
        q -= 1;
        buf[q] = b'0' + (u % 10) as u8;
        u /= 10;
        if u == 0 {
            break;
        }
    }
    if neg {
        q -= 1;
        buf[q] = b'-';
    }
    &buf[q..]
}

/// `lj_strfmt_wuint9`: exactly nine digits.
fn wuint9(out: &mut [u8], p: usize, mut u: u32) -> usize {
    for i in (0..9).rev() {
        out[p + i] = b'0' + (u % 10) as u8;
        u /= 10;
    }
    p + 9
}

fn wuint9_arr(u: u32) -> [u8; 9] {
    let mut a = [0u8; 9];
    wuint9(&mut a, 0, u);
    a
}

/// `lj_fls`: index of the most significant set bit.
#[inline]
fn lj_fls(x: u32) -> u32 {
    31 - x.leading_zeros()
}
/// `lj_ffs`: index of the least significant set bit.
#[inline]
fn lj_ffs(x: u32) -> u32 {
    x.trailing_zeros()
}

// ---------------------------------------------------------------------------
// lj_strfmt_num.c
// ---------------------------------------------------------------------------

#[rustfmt::skip]
static RESCALE_E: [i16; 32] = [
    -308, -289, -270, -250, -231, -212, -193, -173, -154,
    -135, -115, -96, -77, -58, -38, -0, -0, -0, 39, 58,
    77, 96, 116, 135, 154, 174, 193, 212, 231,
    251, 270, 289,
];
#[rustfmt::skip]
static RESCALE_N: [f64; 32] = [
    1e+0308, 1e+0289, 1e+0270, 1e+0250, 1e+0231, 1e+0212, 1e+0193, 1e+0173, 1e+0154,
    1e+0135, 1e+0115, 1e+096, 1e+077, 1e+058, 1e+038, 1e+00, 1e+00, 1e+00, 1e-039, 1e-058,
    1e-077, 1e-096, 1e-0116, 1e-0135, 1e-0154, 1e-0174, 1e-0193, 1e-0212, 1e-0231,
    1e-0251, 1e-0270, 1e-0289,
];

#[rustfmt::skip]
static FOUR_ULP_M_E: [i8; 256] = [
  34, -21, 68, -21, 14, -20, 28, -20, 55, -20, 2, -19, 3, -19, 5, -19, 9, -19,
  -82, -18, 35, -18, 7, -17, -117, -17, 28, -17, 56, -17, 112, -16, -33, -16,
  45, -16, 89, -16, -78, -15, 36, -15, 72, -15, -113, -14, 29, -14, 57, -14,
  114, -13, -28, -13, 46, -13, 91, -12, -74, -12, 37, -12, 73, -12, 15, -11, 3,
  -11, 59, -11, 2, -10, 3, -10, 5, -10, 1, -9, -69, -9, 38, -9, 75, -9, 15, -7,
  3, -7, 6, -7, 12, -6, -17, -7, 48, -7, 96, -7, -65, -6, 39, -6, 77, -6, -103,
  -5, 31, -5, 62, -5, 123, -4, -11, -4, 49, -4, 98, -4, -60, -3, 4, -2, 79, -3,
  16, -2, 32, -2, 63, -2, 2, -1, 25, 0, 5, 1, 1, 2, 2, 2, 4, 2, 8, 2, 16, 2,
  32, 2, 64, 2, -128, 2, 26, 2, 52, 2, 103, 3, -51, 3, 41, 4, 82, 4, -92, 4,
  33, 4, 66, 4, -124, 5, 27, 5, 53, 5, 105, 6, 21, 6, 42, 6, 84, 6, 17, 7, 34,
  7, 68, 7, 2, 8, 3, 8, 6, 8, 108, 9, -41, 9, 43, 10, 86, 9, -84, 10, 35, 10,
  69, 10, -118, 11, 28, 11, 55, 12, 11, 13, 22, 13, 44, 13, 88, 13, -80, 13,
  36, 13, 71, 13, -115, 14, 29, 14, 57, 14, 113, 15, -30, 15, 46, 15, 91, 15,
  19, 16, 37, 16, 73, 16, 2, 17, 3, 17, 6, 17
];

static NDIGITS_DEC_THRESHOLD: [u32; 11] = [
    0,
    9,
    99,
    999,
    9999,
    99999,
    999_999,
    9_999_999,
    99_999_999,
    999_999_999,
    0xffff_ffff,
];

/// Number of digits in the decimal representation of x.
fn ndigits_dec(x: u32) -> u32 {
    let t = ((lj_fls(x | 1) * 77) >> 8) + 1; // 2^8/77 is roughly log2(10)
    t + u32::from(x > NDIGITS_DEC_THRESHOLD[t as usize])
}

const ND_MUL2K_MAX_SHIFT: u32 = 29;

#[inline]
fn nd_mul2k_div1e9(val: u64) -> u32 {
    (val / 1_000_000_000) as u32
}

/// Multiply nd by 2^k and add carry_in (ndlo is assumed to be zero).
fn nd_mul2k(nd: &mut [u32; 64], mut ndhi: u32, mut k: u32, mut carry_in: u32, sf: SFormat) -> u32 {
    let mut ndlo = 0u32;
    let mut start = 1u32;
    // Performance hacks.
    if k > ND_MUL2K_MAX_SHIFT * 2 && strfmt_fp(sf) != strfmt_fp(STRFMT_T_FP_F) {
        start = ndhi.wrapping_sub(strfmt_prec(sf).wrapping_add(17) / 8);
    }
    // Real logic.
    while k >= ND_MUL2K_MAX_SHIFT {
        let mut i = ndlo;
        while i <= ndhi {
            let val = (u64::from(nd[i as usize]) << ND_MUL2K_MAX_SHIFT) | u64::from(carry_in);
            carry_in = nd_mul2k_div1e9(val);
            nd[i as usize] = (val as u32).wrapping_sub(carry_in.wrapping_mul(1_000_000_000));
            i += 1;
        }
        if carry_in != 0 {
            ndhi += 1;
            nd[ndhi as usize] = carry_in;
            carry_in = 0;
            let s = start;
            start = start.wrapping_add(1);
            if s == ndlo {
                ndlo += 1;
            }
        }
        k -= ND_MUL2K_MAX_SHIFT;
    }
    if k != 0 {
        let mut i = ndlo;
        while i <= ndhi {
            let val = (u64::from(nd[i as usize]) << k) | u64::from(carry_in);
            carry_in = nd_mul2k_div1e9(val);
            nd[i as usize] = (val as u32).wrapping_sub(carry_in.wrapping_mul(1_000_000_000));
            i += 1;
        }
        if carry_in != 0 {
            ndhi += 1;
            nd[ndhi as usize] = carry_in;
        }
    }
    ndhi
}

/// Divide nd by 2^k (ndlo is assumed to be zero).
fn nd_div2k(nd: &mut [u32; 64], mut ndhi: u32, mut k: u32, sf: SFormat) -> u32 {
    let mut ndlo = 0u32;
    let mut stop1 = !0u32;
    let mut stop2 = !0u32;
    // Performance hacks.
    if ndhi == 0 {
        if nd[0] == 0 {
            return 0;
        }
        let s = lj_ffs(nd[0]);
        if s >= k {
            nd[0] >>= k;
            return 0;
        }
        nd[0] >>= s;
        k -= s;
    }
    if k > 18 {
        if strfmt_fp(sf) == strfmt_fp(STRFMT_T_FP_F) {
            stop1 = (63i32 - (strfmt_prec(sf) as i32) / 9) as u32;
        } else {
            let floorlog2 = ndhi
                .wrapping_mul(29)
                .wrapping_add(lj_fls(nd[ndhi as usize]))
                .wrapping_sub(k) as i32;
            let floorlog10 = (f64::from(floorlog2) * 0.301_029_995_663_981_14) as i32;
            stop1 = (62i32 + (floorlog10 - strfmt_prec(sf) as i32) / 9) as u32;
            stop2 = 61u32
                .wrapping_add(ndhi)
                .wrapping_sub(((strfmt_prec(sf) as i32) / 8) as u32);
        }
    }
    // Real logic.
    while k >= 9 {
        let mut i = ndhi;
        let mut carry = 0u32;
        loop {
            let val = nd[i as usize];
            nd[i as usize] = (val >> 9).wrapping_add(carry);
            carry = (val & 0x1ff).wrapping_mul(1_953_125);
            if i == ndlo {
                break;
            }
            i = i.wrapping_sub(1) & 0x3f;
        }
        if ndlo != stop1 && ndlo != stop2 {
            if carry != 0 {
                ndlo = ndlo.wrapping_sub(1) & 0x3f;
                nd[ndlo as usize] = carry;
            }
            if nd[ndhi as usize] == 0 {
                ndhi = ndhi.wrapping_sub(1) & 0x3f;
                stop2 = stop2.wrapping_sub(1);
            }
        } else if nd[ndhi as usize] == 0 {
            if ndhi != ndlo {
                ndhi = ndhi.wrapping_sub(1) & 0x3f;
                stop2 = stop2.wrapping_sub(1);
            } else {
                return ndlo;
            }
        }
        k -= 9;
    }
    if k != 0 {
        let mask = (1u32 << k) - 1;
        let mul = 1_000_000_000u32 >> k;
        let mut i = ndhi;
        let mut carry = 0u32;
        loop {
            let val = nd[i as usize];
            nd[i as usize] = (val >> k).wrapping_add(carry);
            carry = (val & mask).wrapping_mul(mul);
            if i == ndlo {
                break;
            }
            i = i.wrapping_sub(1) & 0x3f;
        }
        if carry != 0 {
            ndlo = ndlo.wrapping_sub(1) & 0x3f;
            nd[ndlo as usize] = carry;
        }
    }
    ndlo
}

/// Add m*10^e to nd (assumes ndlo <= e/9 <= ndhi and 0 <= m <= 9).
fn nd_add_m10e(nd: &mut [u32; 64], mut ndhi: u32, m: u8, e: i32) -> u32 {
    let mut i: u32;
    let mut carry: u32;
    if e >= 0 {
        i = e as u32 / 9;
        carry = u32::from(m).wrapping_mul(NDIGITS_DEC_THRESHOLD[(e - i as i32 * 9) as usize] + 1);
    } else {
        let f = (e - 8) / 9;
        i = (64 + f) as u32;
        carry = u32::from(m).wrapping_mul(NDIGITS_DEC_THRESHOLD[(e - f * 9) as usize] + 1);
    }
    loop {
        let mut val = nd[i as usize].wrapping_add(carry);
        if val >= 1_000_000_000 {
            val -= 1_000_000_000;
            nd[i as usize] = val;
            if i == ndhi {
                ndhi = (ndhi + 1) & 0x3f;
                nd[ndhi as usize] = 1;
                break;
            }
            carry = 1;
            i = (i + 1) & 0x3f;
        } else {
            nd[i as usize] = val;
            break;
        }
    }
    ndhi
}

/// Test whether two "nd" values are equal in their most significant digits.
/// `rf` indexes `nd` (the C code passes `nd + 33`).
fn nd_similar(nd: &[u32; 64], mut ndhi: u32, mut rf: usize, hilen: u32, mut prec: u32) -> bool {
    if hilen <= prec {
        if nd[ndhi as usize] != nd[rf] {
            return false;
        }
        prec -= hilen;
        rf -= 1;
        ndhi = ndhi.wrapping_sub(1) & 0x3f;
        if prec >= 9 {
            if nd[ndhi as usize] != nd[rf] {
                return false;
            }
            prec -= 9;
            rf -= 1;
            ndhi = ndhi.wrapping_sub(1) & 0x3f;
        }
    } else {
        prec = prec.wrapping_sub(hilen.wrapping_sub(9));
    }
    debug_assert!(prec < 9, "bad precision {prec}");
    let nd9 = wuint9_arr(nd[ndhi as usize]);
    let ref9 = wuint9_arr(nd[rf]);
    let p = prec as usize;
    nd9[..p] == ref9[..p] && (nd9[p] < b'5') == (ref9[p] < b'5')
}

/// Width-padding loop `while (width-- > len) *p++ = c;`.
#[inline]
fn pad(buf: &mut [u8], p: &mut usize, width: &mut u32, len: u32, c: u8) {
    loop {
        let go = *width > len;
        *width = width.wrapping_sub(1);
        if !go {
            break;
        }
        buf[*p] = c;
        *p += 1;
    }
}

#[inline]
fn hi32(u: u64) -> u32 {
    (u >> 32) as u32
}
#[inline]
fn lo32(u: u64) -> u32 {
    u as u32
}
#[inline]
fn set_hi32(u: &mut u64, hi: u32) {
    *u = (u64::from(hi) << 32) | (*u & 0xffff_ffff);
}

/// Size of the scratch buffer for one number: width and precision are at most
/// 99, `%f` of 1e308 has 309 integer digits, plus the 8-digit overshoot.
const NUMBUF: usize = 640;

/// `lj_strfmt_wfnum`: write formatted floating-point number; returns the length.
#[allow(clippy::cognitive_complexity)]
fn wfnum(buf: &mut [u8; NUMBUF], sf: SFormat, n: f64) -> usize {
    let mut width = strfmt_width(sf);
    let mut prec = strfmt_prec(sf);
    let len: u32;
    let mut t: u64 = n.to_bits();
    let mut p: usize = 0;
    if (hi32(t) << 1) >= 0xffe0_0000 {
        // Handle non-finite values uniformly for %a, %e, %f, %g.
        let mut prefix = 0u8;
        let mut ch: u32 = if sf & STRFMT_F_UPPER != 0 {
            0x0020_2020
        } else {
            0
        };
        if ((hi32(t) & 0x000f_ffff) | lo32(t)) != 0 {
            ch ^= (u32::from(b'n') << 16) | (u32::from(b'a') << 8) | u32::from(b'n');
            if sf & STRFMT_F_SPACE != 0 {
                prefix = b' ';
            }
        } else {
            ch ^= (u32::from(b'i') << 16) | (u32::from(b'n') << 8) | u32::from(b'f');
            if hi32(t) & 0x8000_0000 != 0 {
                prefix = b'-';
            } else if sf & STRFMT_F_PLUS != 0 {
                prefix = b'+';
            } else if sf & STRFMT_F_SPACE != 0 {
                prefix = b' ';
            }
        }
        len = 3 + u32::from(prefix != 0);
        if sf & STRFMT_F_LEFT == 0 {
            pad(buf, &mut p, &mut width, len, b' ');
        }
        if prefix != 0 {
            buf[p] = prefix;
            p += 1;
        }
        buf[p] = (ch >> 16) as u8;
        buf[p + 1] = (ch >> 8) as u8;
        buf[p + 2] = ch as u8;
        p += 3;
    } else if strfmt_fp(sf) == strfmt_fp(STRFMT_T_FP_A) {
        // %a
        let hexdig: &[u8; 18] = if sf & STRFMT_F_UPPER != 0 {
            b"0123456789ABCDEFPX"
        } else {
            b"0123456789abcdefpx"
        };
        let mut e: i32 = ((hi32(t) >> 20) & 0x7ff) as i32;
        let mut prefix = 0u8;
        let mut eprefix = b'+';
        if hi32(t) & 0x8000_0000 != 0 {
            prefix = b'-';
        } else if sf & STRFMT_F_PLUS != 0 {
            prefix = b'+';
        } else if sf & STRFMT_F_SPACE != 0 {
            prefix = b' ';
        }
        let h = hi32(t) & 0xfffff;
        set_hi32(&mut t, h);
        if e != 0 {
            let h = hi32(t) | 0x10_0000;
            set_hi32(&mut t, h);
            e -= 1023;
        } else if (lo32(t) | hi32(t)) != 0 {
            // Non-zero denormal - normalise it.
            let shift = if hi32(t) != 0 {
                20 - lj_fls(hi32(t))
            } else {
                52 - lj_fls(lo32(t))
            };
            e = -1022 - shift as i32;
            t <<= shift;
        }
        // abs(n) == t.u64 * 2^(e - 52)
        // If n != 0, bit 52 of t.u64 is set, and is the highest set bit.
        if (prec as i32) < 0 {
            // Default precision: use smallest precision giving exact result.
            prec = if lo32(t) != 0 {
                13 - lj_ffs(lo32(t)) / 4
            } else {
                5 - lj_ffs(hi32(t) | 0x10_0000) / 4
            };
        } else if prec < 13 {
            // Precision is sufficiently low as to maybe require rounding.
            t = t.wrapping_add(1u64 << (51 - prec * 4));
        }
        if e < 0 {
            eprefix = b'-';
            e = -e;
        }
        len = 5
            + ndigits_dec(e as u32)
            + prec
            + u32::from(prefix != 0)
            + u32::from((prec | (sf & STRFMT_F_ALT)) != 0);
        if sf & (STRFMT_F_LEFT | STRFMT_F_ZERO) == 0 {
            pad(buf, &mut p, &mut width, len, b' ');
        }
        if prefix != 0 {
            buf[p] = prefix;
            p += 1;
        }
        buf[p] = b'0';
        buf[p + 1] = hexdig[17]; // x or X
        p += 2;
        if sf & (STRFMT_F_LEFT | STRFMT_F_ZERO) == STRFMT_F_ZERO {
            pad(buf, &mut p, &mut width, len, b'0');
        }
        buf[p] = b'0' + (hi32(t) >> 20) as u8; // Usually '1', sometimes '0' or '2'.
        p += 1;
        if (prec | (sf & STRFMT_F_ALT)) != 0 {
            // Emit fractional part.
            let q = p + 1 + prec as usize;
            buf[p] = b'.';
            if prec < 13 {
                t >>= 52 - prec * 4;
            } else {
                while prec > 13 {
                    buf[p + prec as usize] = b'0';
                    prec -= 1;
                }
            }
            while prec != 0 {
                buf[p + prec as usize] = hexdig[(t & 15) as usize];
                prec -= 1;
                t >>= 4;
            }
            p = q;
        }
        buf[p] = hexdig[16]; // p or P
        buf[p + 1] = eprefix; // + or -
        p += 2;
        p = wint(buf, p, e);
    } else {
        // %e or %f or %g - begin by converting n to "nd" format.
        let mut nd = [0u32; 64];
        let mut ndhi: u32;
        let mut ndlo: u32;
        let mut i: u32;
        let mut e: i32;
        let mut ndebias: i32;
        let mut prefix = 0u8;
        if hi32(t) & 0x8000_0000 != 0 {
            prefix = b'-';
        } else if sf & STRFMT_F_PLUS != 0 {
            prefix = b'+';
        } else if sf & STRFMT_F_SPACE != 0 {
            prefix = b' ';
        }
        prec = prec.wrapping_add(((prec as i32) >> 31) as u32 & 7); // Default precision is 6.
        if strfmt_fp(sf) == strfmt_fp(STRFMT_T_FP_G) {
            // %g - decrement precision if non-zero (to make it like %e).
            prec = prec.wrapping_sub(1);
            prec ^= ((prec as i32) >> 31) as u32;
        }
        let mut try_rescale = (sf & STRFMT_T_FP_E) != 0 && prec < 14 && n != 0.0;
        // The C code jumps back here (`rescale_failed`) when the rescaled
        // conversion crossed a rounding boundary.
        'conv: loop {
            t = n.to_bits();
            e = ((hi32(t) >> 20) & 0x7ff) as i32;
            ndebias = 0;
            ndhi = 0;
            let mut load_lo = false;
            if try_rescale {
                // Precision is sufficiently low that rescaling will probably work.
                ndebias = i32::from(RESCALE_E[(e >> 6) as usize]);
                if ndebias != 0 {
                    let mut tn = n * RESCALE_N[(e >> 6) as usize];
                    if e == 0 {
                        tn *= 1e10;
                        ndebias -= 10;
                    }
                    t = tn.to_bits();
                    t = t.wrapping_sub(2); // Convert 2ulp below (later we convert 2ulp above).
                    nd[0] = 0x10_0000 | (hi32(t) & 0xfffff);
                    e = ((hi32(t) >> 20) & 0x7ff) as i32 - 1075;
                    load_lo = true;
                }
            }
            if !load_lo {
                nd[0] = hi32(t) & 0xfffff;
                if e == 0 {
                    e += 1;
                } else {
                    nd[0] |= 0x10_0000;
                }
                e -= 1043;
                if lo32(t) != 0 {
                    e -= 32;
                    load_lo = true;
                }
            }
            if load_lo {
                nd[0] = (nd[0] << 3) | (lo32(t) >> 29);
                ndhi = nd_mul2k(&mut nd, ndhi, 29, lo32(t) & 0x1fff_ffff, sf);
            }
            if e >= 0 {
                ndhi = nd_mul2k(&mut nd, ndhi, e as u32, 0, sf);
                ndlo = 0;
            } else {
                ndlo = nd_div2k(&mut nd, ndhi, (-e) as u32, sf);
                if ndhi != 0 && nd[ndhi as usize] == 0 {
                    ndhi -= 1;
                }
            }
            // abs(n) == nd * 10^ndebias (for slightly loose interpretation of ==)
            if sf & STRFMT_T_FP_E != 0 {
                // %e or %g - assume %e and start by calculating nd's exponent (nde).
                let mut eprefix = b'+';
                let mut nde: i32 = -1;
                if ndlo != 0 && nd[ndhi as usize] == 0 {
                    ndhi = 64;
                    loop {
                        ndhi -= 1;
                        if nd[ndhi as usize] != 0 {
                            break;
                        }
                    }
                    nde -= 64 * 9;
                }
                let hilen = ndigits_dec(nd[ndhi as usize]);
                nde = nde.wrapping_add(ndhi.wrapping_mul(9).wrapping_add(hilen) as i32);
                if ndebias != 0 {
                    // Rescaling was performed, but this introduced some error, and might
                    // have pushed us across a rounding boundary. We check whether this
                    // error affected the result by introducing even more error (2ulp in
                    // either direction), and seeing whether a rounding boundary was
                    // crossed. Having already converted the -2ulp case, we save off its
                    // most significant digits, convert the +2ulp case, and compare them.
                    let eidx = e + 70 + i32::from(lo32(t) >= 0xffff_fffe && (!hi32(t) << 12) == 0);
                    debug_assert!((0..128).contains(&eidx), "bad eidx {eidx}");
                    let m_e = &FOUR_ULP_M_E[(eidx * 2) as usize..];
                    nd[33] = nd[ndhi as usize];
                    nd[32] = nd[(ndhi.wrapping_sub(1) & 0x3f) as usize];
                    nd[31] = nd[(ndhi.wrapping_sub(2) & 0x3f) as usize];
                    nd_add_m10e(&mut nd, ndhi, m_e[0] as u8, i32::from(m_e[1]));
                    if !nd_similar(&nd, ndhi, 33, hilen, prec + 1) {
                        try_rescale = false;
                        continue 'conv;
                    }
                }
                if (prec.wrapping_sub(nde as u32) as i32) < (0x3f & ndlo.wrapping_neg() as i32) * 9
                {
                    // Precision is sufficiently low as to maybe require rounding.
                    ndhi = nd_add_m10e(
                        &mut nd,
                        ndhi,
                        5,
                        nde.wrapping_sub(prec as i32).wrapping_sub(1),
                    );
                    nde += i32::from(hilen != ndigits_dec(nd[ndhi as usize]));
                }
                nde += ndebias;
                let mut like_f = false;
                if sf & STRFMT_T_FP_F != 0 {
                    // %g
                    if (prec as i32) >= nde && nde >= -4 {
                        if nde < 0 {
                            ndhi = 0;
                        }
                        prec = prec.wrapping_sub(nde as u32);
                        like_f = true; // goto g_format_like_f
                    } else if sf & STRFMT_F_ALT == 0 && prec != 0 && width > 5 {
                        // Decrease precision in order to strip trailing zeroes.
                        let maxprec = hilen - 1 + (ndhi.wrapping_sub(ndlo) & 0x3f) * 9;
                        if prec >= maxprec {
                            prec = maxprec;
                        } else {
                            ndlo = ndhi
                                .wrapping_sub(((prec.wrapping_sub(hilen) as i32 + 9) / 9) as u32)
                                & 0x3f;
                        }
                        i = prec
                            .wrapping_sub(hilen)
                            .wrapping_sub((ndhi.wrapping_sub(ndlo) & 0x3f) * 9)
                            .wrapping_add(10);
                        let mut tail = wuint9_arr(nd[ndlo as usize]);
                        loop {
                            if prec == 0 {
                                break;
                            }
                            i = i.wrapping_sub(1);
                            if tail[i as usize] != b'0' {
                                break;
                            }
                            prec -= 1;
                            if i == 0 {
                                if ndlo == ndhi {
                                    prec = 0;
                                    break;
                                }
                                ndlo = (ndlo + 1) & 0x3f;
                                tail = wuint9_arr(nd[ndlo as usize]);
                                i = 9;
                            }
                        }
                    }
                }
                if like_f {
                    p = format_like_f(buf, sf, &nd, ndhi, ndlo, prec, width, prefix, true);
                    return p;
                }
                if nde < 0 {
                    // Make nde non-negative.
                    eprefix = b'-';
                    nde = -nde;
                }
                let len = 3
                    + prec
                    + u32::from(prefix != 0)
                    + ndigits_dec(nde as u32)
                    + u32::from(nde < 10)
                    + u32::from((prec | (sf & STRFMT_F_ALT)) != 0);
                if sf & (STRFMT_F_LEFT | STRFMT_F_ZERO) == 0 {
                    pad(buf, &mut p, &mut width, len, b' ');
                }
                if prefix != 0 {
                    buf[p] = prefix;
                    p += 1;
                }
                if sf & (STRFMT_F_LEFT | STRFMT_F_ZERO) == STRFMT_F_ZERO {
                    pad(buf, &mut p, &mut width, len, b'0');
                }
                let q = wint(buf, p + 1, nd[ndhi as usize] as i32);
                buf[p] = buf[p + 1]; // Put leading digit in the correct place.
                if (prec | (sf & STRFMT_F_ALT)) != 0 {
                    // Emit fractional part.
                    buf[p + 1] = b'.';
                    p += 2;
                    prec = prec.wrapping_sub((q - p) as u32); // Account for digits already emitted.
                    p = q;
                    // Then emit chunks of 9 digits (this may emit 8 digits too many).
                    i = ndhi;
                    while (prec as i32) > 0 && i != ndlo {
                        i = i.wrapping_sub(1) & 0x3f;
                        p = wuint9(buf, p, nd[i as usize]);
                        prec = prec.wrapping_sub(9);
                    }
                    if sf & STRFMT_T_FP_F != 0 && sf & STRFMT_F_ALT == 0 {
                        // %g (and not %#g) - strip trailing zeroes.
                        p = (p as isize + ((prec as i32) & ((prec as i32) >> 31)) as isize)
                            as usize;
                        while buf[p - 1] == b'0' {
                            p -= 1;
                        }
                        if buf[p - 1] == b'.' {
                            p -= 1;
                        }
                    } else {
                        // %e (or %#g) - emit trailing zeroes.
                        while (prec as i32) > 0 {
                            buf[p] = b'0';
                            p += 1;
                            prec -= 1;
                        }
                        p = (p as isize + (prec as i32) as isize) as usize;
                    }
                } else {
                    p += 1;
                }
                buf[p] = if sf & STRFMT_F_UPPER != 0 { b'E' } else { b'e' };
                buf[p + 1] = eprefix; // + or -
                p += 2;
                if nde < 10 {
                    buf[p] = b'0'; // Always at least two digits of exponent.
                    p += 1;
                }
                p = wint(buf, p, nde);
                if sf & STRFMT_F_LEFT != 0 {
                    pad(buf, &mut p, &mut width, len, b' ');
                }
                return p;
            }
            // %f (or, shortly, %g in %f style)
            if prec < (0x3f & ndlo.wrapping_neg()) * 9 {
                // Precision is sufficiently low as to maybe require rounding.
                ndhi = nd_add_m10e(
                    &mut nd,
                    ndhi,
                    5,
                    0i32.wrapping_sub(prec as i32).wrapping_sub(1),
                );
            }
            return format_like_f(buf, sf, &nd, ndhi, ndlo, prec, width, prefix, false);
        }
    }
    if sf & STRFMT_F_LEFT != 0 {
        pad(buf, &mut p, &mut width, len, b' ');
    }
    p
}

/// The `g_format_like_f:` tail of `lj_strfmt_wfnum` (shared by `%f` and `%g`
/// in `%f` style), including the final left-justification padding.
#[allow(clippy::too_many_arguments)]
fn format_like_f(
    buf: &mut [u8; NUMBUF],
    sf: SFormat,
    nd: &[u32; 64],
    ndhi: u32,
    mut ndlo: u32,
    mut prec: u32,
    mut width: u32,
    prefix: u8,
    _from_g: bool,
) -> usize {
    let mut p = 0usize;
    let mut i: u32;
    if sf & STRFMT_T_FP_E != 0 && sf & STRFMT_F_ALT == 0 && prec != 0 && width != 0 {
        // Decrease precision in order to strip trailing zeroes.
        if ndlo != 0 {
            // nd has a fractional part; we need to look at its digits.
            let maxprec = (64 - ndlo) * 9;
            if prec >= maxprec {
                prec = maxprec;
            } else {
                ndlo = 64 - (prec + 8) / 9;
            }
            i = prec.wrapping_sub((63 - ndlo) * 9);
            let mut tail = wuint9_arr(nd[ndlo as usize]);
            loop {
                if prec == 0 {
                    break;
                }
                i = i.wrapping_sub(1);
                if tail[i as usize] != b'0' {
                    break;
                }
                prec -= 1;
                if i == 0 {
                    if ndlo == 63 {
                        prec = 0;
                        break;
                    }
                    ndlo += 1;
                    tail = wuint9_arr(nd[ndlo as usize]);
                    i = 9;
                }
            }
        } else {
            // nd has no fractional part, so precision goes straight to zero.
            prec = 0;
        }
    }
    let len = ndhi * 9
        + ndigits_dec(nd[ndhi as usize])
        + prec
        + u32::from(prefix != 0)
        + u32::from((prec | (sf & STRFMT_F_ALT)) != 0);
    if sf & (STRFMT_F_LEFT | STRFMT_F_ZERO) == 0 {
        pad(buf, &mut p, &mut width, len, b' ');
    }
    if prefix != 0 {
        buf[p] = prefix;
        p += 1;
    }
    if sf & (STRFMT_F_LEFT | STRFMT_F_ZERO) == STRFMT_F_ZERO {
        pad(buf, &mut p, &mut width, len, b'0');
    }
    // Emit integer part.
    p = wint(buf, p, nd[ndhi as usize] as i32);
    i = ndhi;
    while i != 0 {
        i -= 1;
        p = wuint9(buf, p, nd[i as usize]);
    }
    if (prec | (sf & STRFMT_F_ALT)) != 0 {
        // Emit fractional part.
        buf[p] = b'.';
        p += 1;
        // Emit chunks of 9 digits (this may emit 8 digits too many).
        while (prec as i32) > 0 && i != ndlo {
            i = i.wrapping_sub(1) & 0x3f;
            p = wuint9(buf, p, nd[i as usize]);
            prec = prec.wrapping_sub(9);
        }
        if sf & STRFMT_T_FP_E != 0 && sf & STRFMT_F_ALT == 0 {
            // %g (and not %#g) - strip trailing zeroes.
            p = (p as isize + ((prec as i32) & ((prec as i32) >> 31)) as isize) as usize;
            while buf[p - 1] == b'0' {
                p -= 1;
            }
            if buf[p - 1] == b'.' {
                p -= 1;
            }
        } else {
            // %f (or %#g) - emit trailing zeroes.
            while (prec as i32) > 0 {
                buf[p] = b'0';
                p += 1;
                prec -= 1;
            }
            p = (p as isize + (prec as i32) as isize) as usize;
        }
    }
    if sf & STRFMT_F_LEFT != 0 {
        pad(buf, &mut p, &mut width, len, b' ');
    }
    p
}

/// `lj_strfmt_putfnum`.
fn putfnum(sb: &mut Vec<u8>, sf: SFormat, n: f64) {
    let mut buf = [0u8; NUMBUF];
    let len = wfnum(&mut buf, sf, n);
    sb.extend_from_slice(&buf[..len]);
}

/// `lj_strfmt_putfxint`: formatted signed/unsigned integer.
fn putfxint(sb: &mut Vec<u8>, mut sf: SFormat, mut k: u64) {
    let mut buf = [0u8; 23];
    let mut q = buf.len();
    let mut prefix: u32 = 0;
    // Figure out signed prefixes.
    if strfmt_type(sf) == STRFMT_INT {
        if (k as i64) < 0 {
            k = (!k).wrapping_add(1);
            prefix = 256 + u32::from(b'-');
        } else if sf & STRFMT_F_PLUS != 0 {
            prefix = 256 + u32::from(b'+');
        } else if sf & STRFMT_F_SPACE != 0 {
            prefix = 256 + u32::from(b' ');
        }
    }
    // Convert number and store to fixed-size buffer in reverse order.
    let mut prec = strfmt_prec(sf);
    if (prec as i32) >= 0 {
        sf &= !STRFMT_F_ZERO;
    }
    if k == 0 {
        // Special-case zero argument.
        if prec != 0 || (sf & (STRFMT_T_OCT | STRFMT_F_ALT)) == (STRFMT_T_OCT | STRFMT_F_ALT) {
            q -= 1;
            buf[q] = b'0';
        }
    } else if sf & (STRFMT_T_HEX | STRFMT_T_OCT) == 0 {
        // Decimal.
        loop {
            q -= 1;
            buf[q] = b'0' + (k % 10) as u8;
            k /= 10;
            if k == 0 {
                break;
            }
        }
    } else if sf & STRFMT_T_HEX != 0 {
        // Hex.
        let hexdig: &[u8; 16] = if sf & STRFMT_F_UPPER != 0 {
            b"0123456789ABCDEF"
        } else {
            b"0123456789abcdef"
        };
        loop {
            q -= 1;
            buf[q] = hexdig[(k & 15) as usize];
            k >>= 4;
            if k == 0 {
                break;
            }
        }
        if sf & STRFMT_F_ALT != 0 {
            prefix = 512 + u32::from(if sf & STRFMT_F_UPPER != 0 { b'X' } else { b'x' });
        }
    } else {
        // Octal.
        loop {
            q -= 1;
            buf[q] = b'0' + (k & 7) as u8;
            k >>= 3;
            if k == 0 {
                break;
            }
        }
        if sf & STRFMT_F_ALT != 0 {
            q -= 1;
            buf[q] = b'0';
        }
    }
    // Calculate sizes.
    let len = (buf.len() - q) as u32;
    if (len as i32) >= (prec as i32) {
        prec = len;
    }
    let mut width = strfmt_width(sf);
    let pprec = prec + (prefix >> 8);
    // Format number with leading/trailing whitespace and zeros.
    let mut w = Vec::with_capacity(width.max(pprec) as usize);
    let push_pad = |w: &mut Vec<u8>, width: &mut u32, c: u8| loop {
        let go = *width > pprec;
        *width = width.wrapping_sub(1);
        if !go {
            break;
        }
        w.push(c);
    };
    if sf & (STRFMT_F_LEFT | STRFMT_F_ZERO) == 0 {
        push_pad(&mut w, &mut width, b' ');
    }
    if prefix != 0 {
        if (prefix as u8) >= b'X' {
            w.push(b'0');
        }
        w.push(prefix as u8);
    }
    if sf & (STRFMT_F_LEFT | STRFMT_F_ZERO) == STRFMT_F_ZERO {
        push_pad(&mut w, &mut width, b'0');
    }
    loop {
        let go = prec > len;
        prec = prec.wrapping_sub(1);
        if !go {
            break;
        }
        w.push(b'0');
    }
    w.extend_from_slice(&buf[q..]); // Add number itself.
    if sf & STRFMT_F_LEFT != 0 {
        push_pad(&mut w, &mut width, b' ');
    }
    sb.extend_from_slice(&w);
}

/// `lj_strfmt_putfnum_int`: number formatted as signed integer.
fn putfnum_int(sb: &mut Vec<u8>, sf: SFormat, n: f64) {
    let k = cvtt_i64(n);
    if i64::from(k as i32) == k && sf == STRFMT_INT {
        // Shortcut for plain %d.
        let mut tmp = itoa_buf();
        sb.extend_from_slice(fmt_i64(&mut tmp, k));
    } else {
        putfxint(sb, sf, k as u64);
    }
}

/// `lj_strfmt_putfnum_uint`: number formatted as unsigned integer.
fn putfnum_uint(sb: &mut Vec<u8>, sf: SFormat, n: f64) {
    let k = if n >= 9_223_372_036_854_775_808.0 {
        cvtt_i64(n - 18_446_744_073_709_551_616.0)
    } else {
        cvtt_i64(n)
    };
    putfxint(sb, sf, k as u64);
}

/// `strfmt_putquotedlen`: `%q`.
fn putquoted(sb: &mut Vec<u8>, s: &[u8]) {
    sb.push(b'"');
    for (idx, &cb) in s.iter().enumerate() {
        let mut c = u32::from(cb);
        if c == u32::from(b'"') || c == u32::from(b'\\') || c == u32::from(b'\n') {
            sb.push(b'\\');
        } else if lj_char_isa(cb, LJ_CHAR_CNTRL) {
            // This can only be 0-31 or 127.
            sb.push(b'\\');
            let next = at(s, idx + 1);
            if c >= 100 || lj_char_isdigit(next) {
                sb.push(b'0' + u8::from(c >= 100));
                if c >= 100 {
                    c -= 100;
                }
                let d = (c * 205) >> 11;
                c -= d * 10;
                sb.push(b'0' + d as u8);
            } else if c >= 10 {
                let d = (c * 205) >> 11;
                c -= d * 10;
                sb.push(b'0' + d as u8);
            }
            c += u32::from(b'0');
        }
        sb.push(c as u8);
    }
    sb.push(b'"');
}

/// `lj_strfmt_putfchar`.
fn putfchar(sb: &mut Vec<u8>, sf: SFormat, c: i32) {
    let mut width = strfmt_width(sf);
    if sf & STRFMT_F_LEFT != 0 {
        sb.push(c as u8);
    }
    loop {
        let go = width > 1;
        width = width.wrapping_sub(1);
        if !go {
            break;
        }
        sb.push(b' ');
    }
    if sf & STRFMT_F_LEFT == 0 {
        sb.push(c as u8);
    }
}

/// `strfmt_putfstrlen`.
fn putfstrlen(sb: &mut Vec<u8>, sf: SFormat, s: &[u8]) {
    let mut width = strfmt_width(sf);
    let mut len = s.len() as u32;
    if len > strfmt_prec(sf) {
        len = strfmt_prec(sf);
    }
    let s = &s[..len as usize];
    if sf & STRFMT_F_LEFT != 0 {
        sb.extend_from_slice(s);
    }
    loop {
        let go = width > len;
        width = width.wrapping_sub(1);
        if !go {
            break;
        }
        sb.push(b' ');
    }
    if sf & STRFMT_F_LEFT == 0 {
        sb.extend_from_slice(s);
    }
}

fn typename(a: &FmtArg<'_>) -> &'static str {
    match a {
        FmtArg::Num(_) => "number",
        FmtArg::Str(_) => "string",
        FmtArg::Bool(_) => "boolean",
        FmtArg::Nil => "nil",
    }
}

/// `lj_lib_checknum`: numbers, or strings convertible to numbers.
fn checknum(a: &FmtArg<'_>, narg: usize) -> f64 {
    match a {
        FmtArg::Num(n) => *n,
        FmtArg::Str(s) => match tonumber(s) {
            Some(n) => n,
            None => {
                crate::lua_error!("bad argument #{narg} to 'format' (number expected, got string)")
            }
        },
        other => crate::lua_error!(
            "bad argument #{narg} to 'format' (number expected, got {})",
            typename(other)
        ),
    }
}

/// `string.format(fmt, ...)` (`lj_strfmt_putarg`). Errors are raised with
/// `lua_error!` using LuaJIT's messages (without the caller position prefix).
pub fn lua_format(fmt: &str, args: &[FmtArg]) -> String {
    let bytes = lua_format_bytes(fmt.as_bytes(), args);
    match String::from_utf8(bytes) {
        Ok(s) => s,
        // `%c`/`%q`/`%.Ns` can produce bytes that are not UTF-8 (documented
        // divergence: lossy re-validation, ui doc §8 rule 4).
        Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
    }
}

/// Byte-level `string.format`.
pub fn lua_format_bytes(fmt: &[u8], args: &[FmtArg]) -> Vec<u8> {
    let mut sb: Vec<u8> = Vec::with_capacity(fmt.len() + 16);
    let mut fs = FormatState {
        fmt,
        p: 0,
        e: fmt.len(),
        str: 0,
        len: 0,
    };
    let mut arg = 0usize; // index into args; Lua argument number is arg + 2
    loop {
        let sf = lj_strfmt_parse(&mut fs);
        if sf == STRFMT_EOF {
            break;
        }
        if sf == STRFMT_LIT {
            sb.extend_from_slice(&fmt[fs.str..fs.str + fs.len]);
        } else if sf == STRFMT_ERR {
            let opt = String::from_utf8_lossy(&fmt[fs.str..fs.str + fs.len]);
            crate::lua_error!("invalid option '{opt}' to 'format'");
        } else {
            let narg = arg + 2;
            let Some(o) = args.get(arg) else {
                crate::lua_error!("bad argument #{narg} to 'format' (value expected)");
            };
            arg += 1;
            match strfmt_type(sf) {
                STRFMT_INT => putfnum_int(&mut sb, sf, checknum(o, narg)),
                STRFMT_UINT => putfnum_uint(&mut sb, sf, checknum(o, narg)),
                STRFMT_NUM => putfnum(&mut sb, sf, checknum(o, narg)),
                STRFMT_STR => {
                    let mut tmp = [0u8; NUMBUF];
                    let s: &[u8] = match o {
                        FmtArg::Str(s) => s.as_bytes(),
                        FmtArg::Num(n) => {
                            let l = wfnum(&mut tmp, STRFMT_G14, *n);
                            &tmp[..l]
                        }
                        FmtArg::Bool(true) => b"true",
                        FmtArg::Bool(false) => b"false",
                        FmtArg::Nil => b"nil",
                    };
                    if sf & STRFMT_T_QUOTED != 0 {
                        putquoted(&mut sb, s); // No formatting.
                    } else {
                        putfstrlen(&mut sb, sf, s);
                    }
                }
                STRFMT_CHAR => putfchar(&mut sb, sf, cvtt_i32(checknum(o, narg))),
                STRFMT_PTR => match o {
                    // lj_obj_ptr is NULL for non-GC values.
                    FmtArg::Num(_) | FmtArg::Bool(_) | FmtArg::Nil => sb.extend_from_slice(b"NULL"),
                    FmtArg::Str(_) => crate::lua_error!(
                        "string.format('%p') of a string: addresses are not reproducible"
                    ),
                },
                _ => unreachable!("bad string format type"),
            }
        }
    }
    sb
}

/// `tostring(number)` / `n .. ""`: `%.14g` (`lj_strfmt_num`).
pub fn tostring_num(x: f64) -> String {
    let mut buf = [0u8; NUMBUF];
    let len = wfnum(&mut buf, STRFMT_G14, x);
    // Always ASCII.
    String::from_utf8(buf[..len].to_vec()).expect("ASCII")
}

/// Append `tostring(x)` to a byte buffer without allocating.
pub fn tostring_num_into(x: f64, out: &mut Vec<u8>) {
    let mut buf = [0u8; NUMBUF];
    let len = wfnum(&mut buf, STRFMT_G14, x);
    out.extend_from_slice(&buf[..len]);
}

// ---------------------------------------------------------------------------
// `..` (LuaConcat)
// ---------------------------------------------------------------------------

/// A value that Lua's `..` accepts: strings and numbers (`tostring`).
pub trait LuaConcat {
    fn concat_into(&self, out: &mut CatBuf);
}

impl LuaConcat for str {
    fn concat_into(&self, out: &mut CatBuf) {
        out.push_str(self);
    }
}
impl LuaConcat for String {
    fn concat_into(&self, out: &mut CatBuf) {
        out.push_str(self);
    }
}
impl LuaConcat for crate::Name {
    fn concat_into(&self, out: &mut CatBuf) {
        out.push_str(self.as_str());
    }
}
impl LuaConcat for std::rc::Rc<str> {
    fn concat_into(&self, out: &mut CatBuf) {
        out.push_str(self);
    }
}
impl LuaConcat for f64 {
    fn concat_into(&self, out: &mut CatBuf) {
        let mut buf = [0u8; NUMBUF];
        let len = wfnum(&mut buf, STRFMT_G14, *self);
        out.push_str(std::str::from_utf8(&buf[..len]).expect("ASCII"));
    }
}
impl LuaConcat for i64 {
    fn concat_into(&self, out: &mut CatBuf) {
        (*self as f64).concat_into(out);
    }
}
impl LuaConcat for i32 {
    fn concat_into(&self, out: &mut CatBuf) {
        f64::from(*self).concat_into(out);
    }
}
impl LuaConcat for u32 {
    fn concat_into(&self, out: &mut CatBuf) {
        f64::from(*self).concat_into(out);
    }
}
impl LuaConcat for usize {
    fn concat_into(&self, out: &mut CatBuf) {
        (*self as f64).concat_into(out);
    }
}
impl<T: LuaConcat + ?Sized> LuaConcat for &T {
    fn concat_into(&self, out: &mut CatBuf) {
        (**self).concat_into(out);
    }
}

// ---------------------------------------------------------------------------
// lj_strscan.c (STRSCAN_OPT_TONUM)
// ---------------------------------------------------------------------------

const STRSCAN_DIG: usize = 1024;
const STRSCAN_MAXDIG: u32 = 800; // 772 + extra are sufficient.
const STRSCAN_DDIG: usize = STRSCAN_DIG / 2;
const STRSCAN_DMASK: u32 = (STRSCAN_DDIG - 1) as u32;
const STRSCAN_MAXEXP: u32 = 1 << 20;

#[inline]
fn dnext(a: u32) -> u32 {
    (a + 1) & STRSCAN_DMASK
}
#[inline]
fn dprev(a: u32) -> u32 {
    a.wrapping_sub(1) & STRSCAN_DMASK
}
#[inline]
fn dlen(lo: u32, hi: u32) -> i32 {
    (lo.wrapping_sub(hi) & STRSCAN_DMASK) as i32
}
#[inline]
fn casecmp(c: u8, k: u8) -> bool {
    (c | 0x20) == k
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum StrScanFmt {
    Int,
    Num,
}

/// `ldexp` (musl `scalbn`; exact, with a single rounding for subnormals).
pub(crate) fn ldexp(mut x: f64, mut n: i32) -> f64 {
    let x1p1023 = f64::from_bits(0x7fe0_0000_0000_0000);
    let x1p53 = f64::from_bits(0x4340_0000_0000_0000);
    let x1p_1022 = f64::from_bits(0x0010_0000_0000_0000);
    if n > 1023 {
        x *= x1p1023;
        n -= 1023;
        if n > 1023 {
            x *= x1p1023;
            n -= 1023;
            if n > 1023 {
                n = 1023;
            }
        }
    } else if n < -1022 {
        // make sure final n < -53 to avoid double rounding in the subnormal range
        x *= x1p_1022 * x1p53;
        n += 1022 - 53;
        if n < -1022 {
            x *= x1p_1022 * x1p53;
            n += 1022 - 53;
            if n < -1022 {
                n = -1022;
            }
        }
    }
    x * f64::from_bits(((0x3ff + n) as u64) << 52)
}

/// Final conversion to double.
fn strscan_double(mut x: u64, ex2: i32, neg: bool) -> f64 {
    // Avoid double rounding for denormals.
    if ex2 <= -1075 && x != 0 {
        let b = (x.leading_zeros() ^ 63) as i32;
        if b + ex2 <= -1023 && b + ex2 >= -1075 {
            let rb = 1u64 << (-1075 - ex2);
            if (x & rb) != 0 && (x & (rb + rb + rb - 1)) != 0 {
                x += rb + rb;
            }
            x &= !(rb + rb - 1);
        }
    }
    // Convert to double using a signed int64_t conversion, then rescale.
    debug_assert!((x as i64) >= 0, "bad double conversion");
    let mut n = x as i64 as f64;
    if neg {
        n = -n;
    }
    if ex2 != 0 {
        n = ldexp(n, ex2);
    }
    n
}

/// `*p != '.' ? *p : *++p`
#[inline]
fn rd_skipdot(s: &[u8], p: &mut usize) -> u8 {
    if at(s, *p) != b'.' {
        at(s, *p)
    } else {
        *p += 1;
        at(s, *p)
    }
}

/// Parse hexadecimal number.
fn strscan_hex(s: &[u8], mut p: usize, mut ex2: i32, neg: bool, dig: u32) -> f64 {
    let mut x: u64 = 0;
    // Scan hex digits.
    let mut i = if dig > 16 { 16 } else { dig };
    while i != 0 {
        let mut d = u32::from(rd_skipdot(s, &mut p));
        if d > u32::from(b'9') {
            d += 9;
        }
        x = (x << 4) + u64::from(d & 15);
        i -= 1;
        p += 1;
    }
    // Summarize rounding-effect of excess digits.
    let mut i = 16;
    while i < dig {
        x |= u64::from(rd_skipdot(s, &mut p) != b'0');
        ex2 += 4;
        i += 1;
        p += 1;
    }
    // Format-specific handling: STRSCAN_INT without STRSCAN_OPT_C becomes NUM.
    // Reduce range, then convert to double.
    if x & 0xc000_0000_0000_0000 != 0 {
        x = (x >> 2) | (x & 3);
        ex2 += 2;
    }
    strscan_double(x, ex2, neg)
}

/// Parse binary number.
fn strscan_bin(s: &[u8], mut p: usize, ex2: i32, neg: bool, dig: u32) -> Option<f64> {
    let mut x: u64 = 0;
    if ex2 != 0 || dig > 64 {
        return None;
    }
    // Scan binary digits.
    let mut i = dig;
    while i != 0 {
        let c = at(s, p);
        if (c & !1) != b'0' {
            return None;
        }
        x = (x << 1) | u64::from(c & 1);
        i -= 1;
        p += 1;
    }
    // Format-specific handling. Dual-number builds scan with
    // STRSCAN_OPT_TOINT: `x < 0x80000000u+neg` becomes an integer, so `-0b0`
    // is integer 0 (+0). Other values are the same number either way.
    if crate::profile::dualnum() && x == 0 {
        return Some(0.0);
    }
    let mut ex2 = ex2;
    // Reduce range, then convert to double.
    if x & 0xc000_0000_0000_0000 != 0 {
        x = (x >> 2) | (x & 3);
        ex2 += 2;
    }
    Some(strscan_double(x, ex2, neg))
}

/// Parse decimal number.
fn strscan_dec(s: &[u8], mut p: usize, mut ex10: i32, neg: bool, mut dig: u32) -> f64 {
    let mut xi = [0u8; STRSCAN_DDIG];
    let mut xip = 0usize;

    if dig != 0 {
        let mut i = dig;
        if i > STRSCAN_MAXDIG {
            ex10 += (i - STRSCAN_MAXDIG) as i32;
            i = STRSCAN_MAXDIG;
        }
        // Scan unaligned leading digit.
        if ((ex10 ^ i as i32) & 1) != 0 {
            xi[xip] = rd_skipdot(s, &mut p) & 15;
            xip += 1;
            i -= 1;
            p += 1;
        }
        // Scan aligned double-digits.
        while i > 1 {
            let d = 10 * (rd_skipdot(s, &mut p) & 15);
            p += 1;
            xi[xip] = d + (rd_skipdot(s, &mut p) & 15);
            xip += 1;
            p += 1;
            i -= 2;
        }
        // Scan and realign trailing digit.
        if i != 0 {
            xi[xip] = 10 * (rd_skipdot(s, &mut p) & 15);
            xip += 1;
            ex10 -= 1;
            dig += 1;
            p += 1;
        }

        // Summarize rounding-effect of excess digits.
        if dig > STRSCAN_MAXDIG {
            loop {
                if rd_skipdot(s, &mut p) != b'0' {
                    xi[xip - 1] |= 1;
                    break;
                }
                p += 1;
                dig -= 1;
                if dig <= STRSCAN_MAXDIG {
                    break;
                }
            }
            dig = STRSCAN_MAXDIG;
        } else {
            // Simplify exponent.
            while ex10 > 0 && dig <= 18 {
                xi[xip] = 0;
                xip += 1;
                ex10 -= 2;
                dig += 2;
            }
        }
    } else {
        // Only got zeros.
        ex10 = 0;
        xi[0] = 0;
    }

    // Fast path for numbers in integer format (but handles e.g. 1e6, too).
    if dig <= 20 && ex10 == 0 {
        let mut x: u64 = u64::from(xi[0]);
        for &d in &xi[1..xip.max(1)] {
            x = x.wrapping_mul(100).wrapping_add(u64::from(d));
        }
        if !(dig == 20 && (xi[0] > 18 || (x as i64) >= 0)) {
            // No overflow?  Plain numbers < 2^63 (STRSCAN_INT → NUM with TONUM).
            if (x as i64) >= 0 {
                let mut n = x as i64 as f64;
                if neg {
                    n = -n;
                }
                return n;
            }
        }
    }

    // Slow non-integer path.
    let mut hi: u32 = 0;
    let mut lo: u32 = xip as u32;
    let mut ex2: i32 = 0;
    let mut idig: i32 = lo as i32 + (ex10 >> 1);

    debug_assert!(lo > 0 && (ex10 & 1) == 0, "bad lo {lo} ex10 {ex10}");

    // Handle simple overflow/underflow.
    if idig > 310 / 2 {
        return if neg {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        };
    } else if idig < -326 / 2 {
        return if neg { -0.0 } else { 0.0 };
    }

    // Scale up until we have at least 17 or 18 integer part digits.
    while idig < 9 && idig < dlen(lo, hi) {
        let mut cy: u32 = 0;
        ex2 -= 6;
        let mut i = dprev(lo);
        loop {
            let mut d: u32 = (u32::from(xi[i as usize]) << 6) + cy;
            cy = ((d >> 2) * 5243) >> 17;
            d -= cy * 100; // Div/mod 100.
            xi[i as usize] = d as u8;
            if i == hi {
                break;
            }
            if d == 0 && i == dprev(lo) {
                lo = i;
            }
            i = dprev(i);
        }
        if cy != 0 {
            hi = dprev(hi);
            if xi[dprev(lo) as usize] == 0 {
                lo = dprev(lo);
            } else if hi == lo {
                lo = dprev(lo);
                xi[dprev(lo) as usize] |= xi[lo as usize];
            }
            xi[hi as usize] = cy as u8;
            idig += 1;
        }
    }

    // Scale down until no more than 17 or 18 integer part digits remain.
    while idig > 9 {
        let mut i = hi;
        let mut cy: u32 = 0;
        ex2 += 6;
        loop {
            cy += u32::from(xi[i as usize]);
            xi[i as usize] = (cy >> 6) as u8;
            cy = 100 * (cy & 0x3f);
            if xi[i as usize] == 0 && i == hi {
                hi = dnext(hi);
                idig -= 1;
            }
            i = dnext(i);
            if i == lo {
                break;
            }
        }
        while cy != 0 {
            if hi == lo {
                xi[dprev(lo) as usize] |= 1;
                break;
            }
            xi[lo as usize] = (cy >> 6) as u8;
            lo = dnext(lo);
            cy = 100 * (cy & 0x3f);
        }
    }

    // Collect integer part digits and convert to rescaled double.
    let mut x: u64 = u64::from(xi[hi as usize]);
    let mut i = dnext(hi);
    loop {
        idig -= 1;
        if !(idig > 0 && i != lo) {
            break;
        }
        x = x * 100 + u64::from(xi[i as usize]);
        i = dnext(i);
    }
    if i == lo {
        loop {
            idig -= 1;
            if idig < 0 {
                break;
            }
            x *= 100;
        }
    } else {
        // Gather round bit from remaining digits.
        x <<= 1;
        ex2 -= 1;
        loop {
            if xi[i as usize] != 0 {
                x |= 1;
                break;
            }
            i = dnext(i);
            if i == lo {
                break;
            }
        }
    }
    strscan_double(x, ex2, neg)
}

/// `lj_strscan_scan(p, len, o, STRSCAN_OPT_TONUM)`: the string-to-number
/// conversion of `tonumber(s)` and of arithmetic on strings.
pub fn tonumber_bytes(s: &[u8]) -> Option<f64> {
    let pe = s.len();
    let mut p = 0usize;
    let mut neg = false;

    // Remove leading space, parse sign and non-numbers.
    if !lj_char_isdigit(at(s, p)) {
        while lj_char_isspace(at(s, p)) {
            p += 1;
        }
        if at(s, p) == b'+' || at(s, p) == b'-' {
            neg = at(s, p) == b'-';
            p += 1;
        }
        if at(s, p) >= b'A' {
            // Parse "inf", "infinity" or "nan".
            let mut tmp = f64::NAN;
            if casecmp(at(s, p), b'i') && casecmp(at(s, p + 1), b'n') && casecmp(at(s, p + 2), b'f')
            {
                tmp = if neg {
                    f64::NEG_INFINITY
                } else {
                    f64::INFINITY
                };
                p += 3;
                if casecmp(at(s, p), b'i')
                    && casecmp(at(s, p + 1), b'n')
                    && casecmp(at(s, p + 2), b'i')
                    && casecmp(at(s, p + 3), b't')
                    && casecmp(at(s, p + 4), b'y')
                {
                    p += 5;
                }
            } else if casecmp(at(s, p), b'n')
                && casecmp(at(s, p + 1), b'a')
                && casecmp(at(s, p + 2), b'n')
            {
                p += 3;
            }
            while lj_char_isspace(at(s, p)) {
                p += 1;
            }
            if at(s, p) != 0 || p < pe {
                return None;
            }
            return Some(tmp);
        }
    }

    // Parse regular number.
    let mut fmt = StrScanFmt::Int;
    let mut cmask = LJ_CHAR_DIGIT;
    let mut base: u32 = 10;
    let mut dp: Option<usize> = None;
    let mut dig: u32 = 0;
    let mut hasdig = false;
    let mut x: u32 = 0;
    let mut ex: i32 = 0;

    // Determine base and skip leading zeros.
    if at(s, p) <= b'0' {
        if at(s, p) == b'0' {
            if casecmp(at(s, p + 1), b'x') {
                base = 16;
                cmask = LJ_CHAR_XDIGIT;
                p += 2;
            } else if casecmp(at(s, p + 1), b'b') {
                base = 2;
                cmask = LJ_CHAR_DIGIT;
                p += 2;
            }
        }
        loop {
            if at(s, p) == b'0' {
                hasdig = true;
            } else if at(s, p) == b'.' {
                if dp.is_some() {
                    return None;
                }
                dp = Some(p);
            } else {
                break;
            }
            p += 1;
        }
    }

    // Preliminary digit and decimal point scan.
    let sp = p;
    loop {
        let c = at(s, p);
        if lj_char_isa(c, cmask) {
            x = x.wrapping_mul(10).wrapping_add(u32::from(c & 15)); // For fast path below.
            dig += 1;
        } else if c == b'.' {
            if dp.is_some() {
                return None;
            }
            dp = Some(p);
        } else {
            break;
        }
        p += 1;
    }
    if !(hasdig || dig != 0) {
        return None;
    }

    // Handle decimal point.
    if let Some(dpv) = dp {
        if base == 2 {
            return None;
        }
        fmt = StrScanFmt::Num;
        if dig != 0 {
            ex = (dpv as isize - (p as isize - 1)) as i32;
            let mut d = p - 1;
            while ex < 0 {
                let c = at(s, d);
                d = d.wrapping_sub(1);
                if c != b'0' {
                    break;
                }
                ex += 1;
                dig -= 1; // Skip trailing zeros.
            }
            if ex <= -(STRSCAN_MAXEXP as i32) {
                return None;
            }
            if base == 16 {
                ex *= 4;
            }
        }
    }

    // Parse exponent.
    if base >= 10 && casecmp(at(s, p), if base == 16 { b'p' } else { b'e' }) {
        let mut negx = false;
        fmt = StrScanFmt::Num;
        p += 1;
        if at(s, p) == b'+' || at(s, p) == b'-' {
            negx = at(s, p) == b'-';
            p += 1;
        }
        if !lj_char_isdigit(at(s, p)) {
            return None;
        }
        let mut xx: u32 = u32::from(at(s, p) & 15);
        p += 1;
        while lj_char_isdigit(at(s, p)) {
            xx = xx * 10 + u32::from(at(s, p) & 15);
            if xx >= STRSCAN_MAXEXP {
                return None;
            }
            p += 1;
        }
        ex = ex.wrapping_add(if negx {
            (!xx).wrapping_add(1) as i32
        } else {
            xx as i32
        });
    }

    // Parse suffix. Every C suffix (I, U, L, LL, ULL, ...) needs an option
    // tonumber() does not pass, so any non-space character is an error.
    if at(s, p) != 0 {
        if casecmp(at(s, p), b'i') {
            return None; // !(opt & STRSCAN_OPT_IMAG)
        } else if fmt == StrScanFmt::Int {
            let c = at(s, p);
            if casecmp(c, b'u') || casecmp(c, b'l') {
                return None;
            }
        }
        while lj_char_isspace(at(s, p)) {
            p += 1;
        }
        if at(s, p) != 0 {
            return None;
        }
    }
    if p < pe {
        return None;
    }

    // Fast path for decimal 32 bit integers.
    if fmt == StrScanFmt::Int
        && base == 10
        && (dig < 10
            || (dig == 10 && at(s, sp) <= b'2' && x < 0x8000_0000u32.wrapping_add(u32::from(neg))))
    {
        // STRSCAN_OPT_TONUM
        return Some(if neg { -f64::from(x) } else { f64::from(x) });
    }

    // Dispatch to base-specific parser.
    if base == 16 {
        Some(strscan_hex(s, sp, ex, neg, dig))
    } else if base == 2 {
        strscan_bin(s, sp, ex, neg, dig)
    } else {
        Some(strscan_dec(s, sp, ex, neg, dig))
    }
}

/// `tonumber(s)` for a string argument (`lj_strscan_num`): decimal, hex
/// (incl. hex floats `0x1p4`), binary `0b101`, `inf`/`nan`, surrounding spaces.
pub fn tonumber(s: &str) -> Option<f64> {
    tonumber_bytes(s.as_bytes())
}

/// `tonumber(s, base)` (lib_base.c). `base == 10` takes the `tonumber(s)` path;
/// other bases use C `strtoul` (glibc semantics, including the `0x` prefix in
/// base 16 and saturation at 2^64-1).
pub fn tonumber_base(s: &str, base: u32) -> Option<f64> {
    tonumber_base_bytes(s.as_bytes(), base)
}

/// Byte-level [`tonumber_base`].
pub fn tonumber_base_bytes(s: &[u8], base: u32) -> Option<f64> {
    if base == 10 {
        return tonumber_bytes(s);
    }
    if !(2..=36).contains(&base) {
        crate::lua_error!("bad argument #2 to 'tonumber' (base out of range)");
    }
    let mut p = 0usize;
    let mut neg = false;
    while lj_char_isspace(at(s, p)) {
        p += 1;
    }
    if at(s, p) == b'-' {
        p += 1;
        neg = true;
    } else if at(s, p) == b'+' {
        p += 1;
    }
    if !lj_char_isa(at(s, p), LJ_CHAR_ALNUM) {
        return None;
    }
    // strtoul(p, &ep, base)
    let digit = |c: u8| -> Option<u32> {
        let v = match c {
            b'0'..=b'9' => u32::from(c - b'0'),
            b'a'..=b'z' => u32::from(c - b'a') + 10,
            b'A'..=b'Z' => u32::from(c - b'A') + 10,
            _ => return None,
        };
        (v < base).then_some(v)
    };
    let save0 = p;
    let mut q = p;
    if base == 16 && at(s, q) == b'0' && (at(s, q + 1) | 0x20) == b'x' {
        q += 2;
    }
    let start = q;
    let mut ul: u64 = 0;
    let mut overflow = false;
    while let Some(d) = digit(at(s, q)) {
        match ul
            .checked_mul(u64::from(base))
            .and_then(|v| v.checked_add(u64::from(d)))
        {
            Some(v) => ul = v,
            None => overflow = true,
        }
        q += 1;
    }
    let ep = if q == start {
        // No digits: "0x" followed by a non-hex char converts the "0".
        if start >= save0 + 2 {
            save0 + 1
        } else {
            save0
        }
    } else {
        q
    };
    if overflow {
        ul = u64::MAX;
    }
    if ep == save0 {
        return None;
    }
    let mut ep = ep;
    while lj_char_isspace(at(s, ep)) {
        ep += 1;
    }
    if at(s, ep) != 0 {
        return None;
    }
    if crate::profile::dualnum() && ul < 0x8000_0000u64 + u64::from(neg) {
        // setintV((int32_t)(neg ? ~ul+1u : ul)): an integer, so "-0" is +0.
        let i = if neg {
            (!(ul as u32)).wrapping_add(1) as i32
        } else {
            ul as i32
        };
        return Some(f64::from(i));
    }
    let mut n = ul as f64;
    if neg {
        n = -n;
    }
    Some(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tostring_basics() {
        assert_eq!(tostring_num(1.0), "1");
        assert_eq!(tostring_num(-0.0), "-0");
        assert_eq!(tostring_num(0.1), "0.1");
        assert_eq!(tostring_num(1e15), "1e+15");
        // LuaJIT rounds the exact tie 1.2345678901234|5e14 up (nd_add_m10e), unlike glibc printf.
        assert_eq!(tostring_num(123456789012345.0), "1.2345678901235e+14");
        assert_eq!(tostring_num(f64::INFINITY), "inf");
        assert_eq!(tostring_num(f64::NEG_INFINITY), "-inf");
        assert_eq!(tostring_num(f64::NAN), "nan");
        assert_eq!(tostring_num(1.0 / 3.0), "0.33333333333333");
        assert_eq!(tostring_num(5e-324), "4.9406564584125e-324");
    }

    #[test]
    fn format_basics() {
        assert_eq!(lfmt!("%.2f", 1.005), "1.00");
        assert_eq!(lfmt!("%d", 3.7), "3");
        assert_eq!(lfmt!("%5.1f%%", 12.345), " 12.3%");
        assert_eq!(lfmt!("%s=%s", "a", 1.5), "a=1.5");
        assert_eq!(lfmt!("%x %X %o", 255.0, 255.0, 8.0), "ff FF 10");
        assert_eq!(lfmt!("%+05d|%-5d|", 42.0, 42.0), "+0042|42   |");
        assert_eq!(
            lfmt!("%g %g %g", 1e20, 0.0001, 100000.0),
            "1e+20 0.0001 100000"
        );
        assert_eq!(lfmt!("%e", 12345.678), "1.234568e+04");
        assert_eq!(lfmt!("%q", "a\"b\n\x01"), "\"a\\\"b\\\n\\1\"");
        assert_eq!(lfmt!("%a", 1.0), "0x1p+0");
        assert_eq!(lfmt!("%c%c", 72.0, 105.0), "Hi");
    }

    #[test]
    fn tonumber_basics() {
        assert_eq!(tonumber(" 12 "), Some(12.0));
        assert_eq!(tonumber("-1.5"), Some(-1.5));
        assert_eq!(tonumber("0x10"), Some(16.0));
        assert_eq!(tonumber("1e3"), Some(1000.0));
        assert_eq!(tonumber("0x1p4"), Some(16.0));
        assert_eq!(tonumber(".5"), Some(0.5));
        assert_eq!(tonumber("5."), Some(5.0));
        assert_eq!(tonumber("0.1"), Some(0.1));
        assert_eq!(tonumber("abc"), None);
        assert_eq!(tonumber(""), None);
        assert_eq!(tonumber("1e"), None);
        assert_eq!(tonumber("12u"), None);
        assert!(tonumber("nan").unwrap().is_nan());
        assert_eq!(tonumber("-inf"), Some(f64::NEG_INFINITY));
        assert_eq!(tonumber_base("ff", 16), Some(255.0));
        assert_eq!(tonumber_base("0x1f", 16), Some(31.0));
        assert_eq!(tonumber_base("-z", 36), Some(-35.0));
        assert_eq!(tonumber_base("2", 2), None);
    }
}
