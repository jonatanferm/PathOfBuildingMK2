//! @pob-file Modules/Common.lua @16de4b82d
//! UI-only byte-string helpers (ui doc §8 rules 1, 4, 5; §12.3 U5). Owner: U5.
//!
//! `pob_lua` (the calc plan's crate) holds Lua's string semantics; this file adds what UI ports
//! need on top of it:
//! * expression-position forms (§8 rule 5: a call inside an expression keeps only its first
//!   value): [`gsub`], [`gsub_bytes_pat`], [`lmatch`];
//! * Lua-number index arguments: [`lua_int`] (`lj_num2int`, what `string.sub`/`find`/`rep` do to
//!   a number argument) and [`lua_integer`] (`lua_tointeger`, what C modules see);
//! * caret-safe sub/insert for EditControl: [`sub`]/[`sub_bytes`] with `f64` indices, and
//!   [`LBuf`]/[`splice`]/[`insert`], which concatenate Lua byte slices **as bytes** and validate
//!   once at the end, so `buf:sub(1, c - 1) .. text .. buf:sub(c)` is byte-exact whenever the Lua
//!   result is valid UTF-8 (a cut through a multi-byte character that the concatenation
//!   rejoins is not mangled; only a result Lua itself would hold as invalid UTF-8 is replaced
//!   lossily, ui doc §8 rule 4);
//! * `tostring` glue: [`tostring`] over [`LuaToString`] values and [`lcat!`](crate::lcat) (Lua
//!   `..` into a `String`; `pob_lua::cat!` interns a `Name` and is for calc keys);
//! * the `lua-utf8` C module PoB loads (luautf8 `lutf8lib.c`), in [`utf8`];
//! * `formatNumSep` (Common.lua:803-828), PoB's only other `lua-utf8` user: [`format_num_sep`].
//!
//! # §8 audit (U5 acceptance: every helper resolves)
//!
//! Each Lua helper named in ui doc §7.4 and §8, and each Lua library function the UI files
//! (`Classes/*.lua`, `Modules/{Main,Build,BuildList,Common}.lua`, `Launch.lua`) call, resolves to
//! one Rust function. `tests/luastr.rs::audit_resolves` names every path below, so a rename
//! breaks the build.
//!
//! | Lua | Rust |
//! |---|---|
//! | `math.floor` / `math.ceil` | `f64::floor` / `f64::ceil` (= `pob_lua::math::{m_floor, m_ceil}`) |
//! | `math.min` / `math.max` (2 args; n args) | `pob_lua::math::{m_min, m_max}`; `{m_min_n, m_max_n}` |
//! | `math.abs`, `math.sqrt`, `math.modf`, `math.log`, `math.pow`, `x ^ y` | `pob_lua::math::{m_abs, m_sqrt, m_modf, m_log, m_pow, lpow}` |
//! | `math.huge`, `math.pi` | `pob_lua::math::{m_huge, m_pi}` |
//! | `math.sin` / `cos` / `tan` / `rad` | `f64::{sin, cos, tan, to_radians}` (libm, like LuaJIT's `lib_math.c`) |
//! | `math.random` / `randomseed` | **gap** (CCR-0033 item 3) |
//! | `a % b` | `pob_lua::fmod_lua` |
//! | `round(v, dec)` and the other Common.lua rounding helpers | `pob_lua::round`, `pob_lua::math::*` |
//! | `bit.band/bor/bxor/bnot/lshift/rshift/tobit` | `pob_lua::bit::*` |
//! | `tostring(n)`, `n .. ""` | `pob_lua::tostring`; [`tostring`] for mixed values; [`lcat!`](crate::lcat) |
//! | `tonumber(s)` / `tonumber(s, b)` | `pob_lua::tonumber` / `pob_lua::tonumber_base` |
//! | `string.format(f, ..)` | `pob_lua::lformat!` (`lfmt!`) |
//! | `s:gsub(p, r)` (expression position / all values) | [`gsub`] / `pob_lua::gsub`, `pob_lua::{gsub_fn, gsub_table}` |
//! | `s:gsub("[\128-\255]", r)` (a pattern that is not UTF-8 text) | [`gsub_bytes_pat`] / `pob_lua::pattern::gsub_bytes_with` |
//! | `s:match(p)` (expression position / all values) | [`lmatch`] / `pob_lua::lmatch` |
//! | `s:find(p, i, plain)` | `pob_lua::find` (number `i`: [`lua_int`]) |
//! | `s:gmatch(p)` | `pob_lua::gmatch` |
//! | `s:sub(i, j)` | [`sub`] / [`sub_bytes`] (`f64` indices); `pob_lua::string::sub` (`i64`) |
//! | `a:sub(..) .. b .. a:sub(..)` (EditControl buffers) | [`LBuf`], [`splice`], [`insert`] |
//! | `#s` | [`len`] |
//! | `s:lower()` / `s:upper()` / `s:rep(n)` / `s:rep(n, sep)` / `s:byte(i)` / `string.char` / `s:reverse()` | `pob_lua::string::{lower, upper, rep, rep_sep, byte, char_bytes, reverse_bytes}` |
//! | `StripEscapes`, `sanitiseText`, `string:split` | `pob_lua::string::{StripEscapes, sanitiseText, split}` |
//! | Lua patterns in filters (`"[" .. filter .. "]"`, SearchHost, CalcFormat, WrapString) | `pob_lua::pattern` |
//! | `table.sort(t, f)` | `pob_lua::sort` |
//! | `pairs` over integer keys | `pob_lua::LjIntMap` |
//! | `ipairs` over a live table | `pob_lua::ipairs::ipairs_live` |
//! | `error(msg)` / `assert(x, msg)` | `pob_lua::lua_error!` (re-exported as `pob_ui_core::lua_error`) |
//! | `utf8.sub/reverse/find/match/gsub/next` (`lua-utf8`) | [`utf8`] |
//! | `formatNumSep(str)` | [`format_num_sep`], [`format_num_sep_main`] |
//!
//! Gaps and contract notes are filed in `docs/port/ccr/0033-u5-lua-helper-audit.md` (CCR-0033).

use std::borrow::Cow;

use crate::main_core::MainCore;

pub mod utf8;

#[doc(hidden)]
pub use pob_lua as __pob_lua;

// ---------------------------------------------------------------------------------------------
// Expression-position forms (§8 rule 5; CCR-0013 B1)
// ---------------------------------------------------------------------------------------------

/// `s:gsub(p, repl)` in expression position: the string only.
pub fn gsub(s: &str, p: &str, repl: &str) -> String {
    pob_lua::gsub(s, p, repl, None).0
}

/// `s:gsub(p, repl)` in expression position with a pattern that is not UTF-8 text, e.g.
/// `text:gsub("[\128-\255]", "?")` (EditControl.lua:539, the paste filter): `p` is the Lua
/// pattern's bytes. The result is re-validated lossily only if Lua's bytes are not UTF-8.
pub fn gsub_bytes_pat(s: &str, p: &[u8], repl: &str) -> String {
    let (b, _) = pob_lua::pattern::gsub_bytes_with(
        s.as_bytes(),
        p,
        pob_lua::pattern::Repl::Str(repl.as_bytes()),
        None,
    );
    LBuf(b).into_string()
}

/// `s:match(p)` in expression position: the first capture (or the whole match) as a string; nil
/// becomes None. A position capture `()` shows as its number (`Capture::to_str`); callers that
/// need the number use `pob_lua::lmatch` directly.
pub fn lmatch(s: &str, p: &str) -> Option<String> {
    let caps = pob_lua::lmatch(s, p, None)?;
    caps.first().map(|c| c.to_str().into_owned())
}

// ---------------------------------------------------------------------------------------------
// Lua numbers as integer arguments
// ---------------------------------------------------------------------------------------------

/// `lj_num2int(n)`, the conversion LuaJIT's string library applies to a number argument
/// (`lj_lib_checkint`/`lj_lib_optint`; the `string.sub` fast path's `cvttsd2si r32`): truncate
/// toward zero; NaN and values outside int32 give `i32::MIN` (x64 "integer indefinite", the
/// shipped profile, CCR-0001). `pob_lua::lj_num2int` (CCR-0033 §1).
pub use pob_lua::lj_num2int as lua_int;

/// `lua_tointeger(L, i)` on 64-bit LuaJIT, i.e. `(lua_Integer)n` (x64 `cvttsd2si r64`): what a C
/// module (`lua-utf8`) sees through `luaL_checkinteger`/`luaL_optinteger`. NaN and values
/// outside int64 give `i64::MIN`. `pob_lua::lua_tointeger` (CCR-0033 §1).
pub use pob_lua::lua_tointeger as lua_integer;

// ---------------------------------------------------------------------------------------------
// Byte-exact sub and concatenation (EditControl buffers)
// ---------------------------------------------------------------------------------------------

/// `#s`: the byte length as a Lua number.
#[inline]
#[must_use]
pub fn len(s: &str) -> f64 {
    s.len() as f64
}

/// `s:sub(i, j)` on bytes with Lua-number indices (1-based, negative from the end, `j` nil =
/// -1). Exact: the bytes Lua's result holds.
#[must_use]
pub fn sub_bytes(s: &[u8], i: f64, j: Option<f64>) -> &[u8] {
    pob_lua::string::sub_bytes(s, i64::from(lua_int(i)), j.map(|j| i64::from(lua_int(j))))
}

/// `s:sub(i, j)` with Lua-number indices. Borrowed when the byte range falls on character
/// boundaries; a range that cuts a multi-byte character is re-validated lossily (§8 rule 4).
/// Use [`sub_bytes`] (or [`LBuf::push_sub`] when the piece is concatenated) where the exact
/// bytes matter.
#[must_use]
pub fn sub(s: &str, i: f64, j: Option<f64>) -> Cow<'_, str> {
    pob_lua::string::sub(s, i64::from(lua_int(i)), j.map(|j| i64::from(lua_int(j))))
}

/// A Lua string being built by `..`, as bytes. Pieces are appended exactly (including
/// [`sub_bytes`] cuts through multi-byte characters); [`LBuf::into_string`] validates once.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LBuf(pub Vec<u8>);

impl LBuf {
    #[must_use]
    pub fn new() -> Self {
        Self(Vec::new())
    }

    /// `.. s`
    pub fn push_str(&mut self, s: &str) -> &mut Self {
        self.0.extend_from_slice(s.as_bytes());
        self
    }

    /// `.. bytes`
    pub fn push_bytes(&mut self, b: &[u8]) -> &mut Self {
        self.0.extend_from_slice(b);
        self
    }

    /// `.. s:sub(i, j)`
    pub fn push_sub(&mut self, s: &str, i: f64, j: Option<f64>) -> &mut Self {
        self.0.extend_from_slice(sub_bytes(s.as_bytes(), i, j));
        self
    }

    /// `.. n` (a number: `tostring`, `%.14g`)
    pub fn push_num(&mut self, n: f64) -> &mut Self {
        pob_lua::format::tostring_num_into(n, &mut self.0);
        self
    }

    /// The byte length (`#` of the concatenation).
    #[must_use]
    pub fn len(&self) -> f64 {
        self.0.len() as f64
    }

    /// The bytes as a `String`: exact when they are valid UTF-8; otherwise the invalid
    /// sequences are replaced lossily (ui doc §8 rule 4).
    #[must_use]
    pub fn into_string(self) -> String {
        match String::from_utf8(self.0) {
            Ok(s) => s,
            Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
        }
    }
}

/// `buf:sub(1, left - 1) .. text .. buf:sub(right)` (EditControl.lua:152 `ReplaceSel`; with
/// `text = ""`, the deletions at :627 and :652), concatenated as bytes. The index arithmetic
/// (`left - 1`) is done on Lua numbers, as in Lua.
#[must_use]
pub fn splice(buf: &str, left: f64, right: f64, text: &str) -> String {
    let mut b = LBuf::new();
    b.push_sub(buf, 1.0, Some(left - 1.0))
        .push_str(text)
        .push_sub(buf, right, None);
    b.into_string()
}

/// `buf:sub(1, caret - 1) .. text .. buf:sub(caret)` (EditControl.lua:174 `Insert`).
#[must_use]
pub fn insert(buf: &str, caret: f64, text: &str) -> String {
    splice(buf, caret, caret, text)
}

// ---------------------------------------------------------------------------------------------
// tostring glue
// ---------------------------------------------------------------------------------------------

/// A Rust value that stands for a Lua value Lua's `tostring` accepts without a metatable.
pub trait LuaToString {
    /// Append `tostring(self)`.
    fn lua_tostring_into(&self, out: &mut String);
}

impl LuaToString for f64 {
    fn lua_tostring_into(&self, out: &mut String) {
        out.push_str(&pob_lua::tostring(*self));
    }
}
impl LuaToString for i32 {
    fn lua_tostring_into(&self, out: &mut String) {
        f64::from(*self).lua_tostring_into(out);
    }
}
impl LuaToString for u32 {
    fn lua_tostring_into(&self, out: &mut String) {
        f64::from(*self).lua_tostring_into(out);
    }
}
impl LuaToString for i64 {
    fn lua_tostring_into(&self, out: &mut String) {
        (*self as f64).lua_tostring_into(out);
    }
}
impl LuaToString for usize {
    fn lua_tostring_into(&self, out: &mut String) {
        (*self as f64).lua_tostring_into(out);
    }
}
impl LuaToString for str {
    fn lua_tostring_into(&self, out: &mut String) {
        out.push_str(self);
    }
}
impl LuaToString for String {
    fn lua_tostring_into(&self, out: &mut String) {
        out.push_str(self);
    }
}
impl LuaToString for bool {
    fn lua_tostring_into(&self, out: &mut String) {
        out.push_str(if *self { "true" } else { "false" });
    }
}
/// `None` is `nil`.
impl<T: LuaToString> LuaToString for Option<T> {
    fn lua_tostring_into(&self, out: &mut String) {
        match self {
            Some(v) => v.lua_tostring_into(out),
            None => out.push_str("nil"),
        }
    }
}
impl<T: LuaToString + ?Sized> LuaToString for &T {
    fn lua_tostring_into(&self, out: &mut String) {
        (**self).lua_tostring_into(out);
    }
}

/// Lua `tostring(v)` for strings, numbers (`%.14g`), booleans and nil (`None`), e.g.
/// `self.buf = tostring(text)` (EditControl.lua:102) where `text` is a string or a number.
#[must_use]
pub fn tostring<T: LuaToString + ?Sized>(v: &T) -> String {
    let mut s = String::new();
    v.lua_tostring_into(&mut s);
    s
}

/// Lua `a .. b .. c` into a `String`: strings as they are, numbers through `tostring`
/// (`%.14g`), via `pob_lua::LuaConcat` (so booleans and nil do not compile, as Lua's `..` raises
/// on them). `pob_lua::cat!` is the interning form for calc keys; UI text uses this one.
#[macro_export]
macro_rules! lcat {
    ($($p:expr),+ $(,)?) => {{
        let mut b = $crate::luastr::__pob_lua::name::CatBuf::new();
        $( $crate::luastr::__pob_lua::format::LuaConcat::concat_into(&$p, &mut b); )+
        ::std::string::String::from(b.as_str())
    }};
}

// ---------------------------------------------------------------------------------------------
// formatNumSep (Common.lua:803-828)
// ---------------------------------------------------------------------------------------------

// Formats "1234.56" -> "1,234.5"
/// `formatNumSep(str)` with `main.showThousandsSeparators`, `main.thousandsSeparator` and
/// `main.decimalSeparator` passed in. Returns both values of the Lua `return string.gsub(..)`
/// (the result and the number of numbers formatted); callers in expression position take `.0`
/// (or use [`format_num_sep_main`]). A number argument is passed as `pob_lua::tostring(n)`
/// (`string.gsub` coerces it the same way).
///
/// DIVERGENCE(global): Lua assigns the global `rev1kSep` (Common.lua:812); nothing else reads
/// it, so the port keeps it local.
/// lua: Common.lua:803-828
/// @pob Modules/Common.lua:803-828 h=6dee6380
pub fn format_num_sep(
    s: &str,
    show_thousands_separators: bool,
    thousands_separator: &str,
    decimal_separator: &str,
) -> (String, usize) {
    // L804
    pob_lua::gsub_fn(
        s,
        "(%^?x?%x?%x?%x?%x?%x?%x?-?%d+%.?%d+)",
        |c| {
            let m = c[0].to_str();
            let m: &str = &m;
            // L805
            let colour = lmatch(m, "(^x%x%x%x%x%x%x)")
                .or_else(|| lmatch(m, "(%^%d)"))
                .unwrap_or_default();
            // L806
            let str_ = gsub(&gsub(m, "(^x%x%x%x%x%x%x)", ""), "(%^%d)", "");
            // L807: return if we have an invalid color code or a completely stripped number.
            if str_.is_empty() || (colour.is_empty() && pob_lua::lmatch(m, "%^", None).is_some()) {
                return Some(m.to_owned());
            }
            // L810
            let found = pob_lua::find(&str_, "(-?)(%d+)(%.?%d*)", None, false);
            let (minus, integer, fraction): (Option<Vec<u8>>, Option<Vec<u8>>, Option<Vec<u8>>) =
                match &found {
                    Some((_x, _y, caps)) => (
                        Some(caps[0].as_bytes().to_vec()),
                        Some(caps[1].as_bytes().to_vec()),
                        Some(caps[2].as_bytes().to_vec()),
                    ),
                    None => (None, None, None),
                };
            // `utf8.reverse(nil)` raises in luaL_checklstring (unreachable for the strings the
            // pattern above can capture; kept so a nil is not silently formatted).
            let need = |v: Option<Vec<u8>>| {
                v.unwrap_or_else(|| {
                    pob_lua::lua_error!("bad argument #1 to 'reverse' (string expected, got nil)")
                })
            };
            let mut integer = need(integer);
            // L811
            if show_thousands_separators {
                // L812
                let rev1k_sep = utf8::reverse(thousands_separator.as_bytes(), false);
                // L813: the outer reverse receives gsub's count as `lax` (a number: truthy).
                let mut repl = b"%1".to_vec();
                repl.extend_from_slice(&rev1k_sep);
                let (g, _n) = utf8::gsub(&utf8::reverse(&integer, false), b"(%d%d%d)", &repl, None);
                integer = utf8::reverse(&g, true);
                // There will be leading separators if the number of digits are divisible by 3
                // This checks for their presence and removes them
                // Don't use patterns here because thousandsSeparator can be a pattern control character, and will crash if used
                // L817
                if !thousands_separator.is_empty() {
                    // L818
                    let thousands_separator_pos =
                        utf8::find(&integer, &rev1k_sep, Some(1.0), true).map(|f| f.0);
                    // L819
                    if thousands_separator_pos == Some(1.0) {
                        // L820
                        integer = utf8::sub(&integer, 2.0, None).to_vec();
                    }
                }
            } else {
                // L824
                let (g, _n) = utf8::gsub(&utf8::reverse(&integer, false), b"(%d%d%d)", b"%1", None);
                integer = utf8::reverse(&g, true);
            }
            // L826
            let fraction = fraction.unwrap_or_default();
            let mut out = LBuf::new();
            out.push_str(&colour)
                .push_bytes(&minus.unwrap_or_default())
                .push_bytes(&integer)
                .push_bytes(&utf8::gsub(&fraction, b"%.", decimal_separator.as_bytes(), None).0);
            Some(out.into_string())
        },
        None,
    )
}

/// `formatNumSep(str)` in expression position, reading the separators from `main` (`cx.main`).
pub fn format_num_sep_main(main: &MainCore, s: &str) -> String {
    format_num_sep(
        s,
        main.show_thousands_separators,
        &main.thousands_separator,
        &main.decimal_separator,
    )
    .0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn num_args() {
        assert_eq!(lua_int(2.9), 2);
        assert_eq!(lua_int(-2.9), -2);
        assert_eq!(lua_int(f64::NAN), i32::MIN);
        assert_eq!(lua_int(3e9), i32::MIN);
        assert_eq!(lua_integer(3e9), 3_000_000_000);
        assert_eq!(lua_integer(f64::INFINITY), i64::MIN);
    }

    #[test]
    fn splice_rejoins_cut_characters() {
        // "aéb": é is bytes 2..3. Cutting at 3 and re-inserting nothing rejoins it exactly.
        assert_eq!(splice("a\u{e9}b", 3.0, 3.0, ""), "a\u{e9}b");
        assert_eq!(insert("ab", 2.0, "\u{20ac}"), "a\u{20ac}b");
        // Deleting one byte of a two-byte character leaves invalid UTF-8: lossy.
        assert_eq!(splice("a\u{e9}b", 3.0, 4.0, ""), "a\u{fffd}b");
        assert_eq!(sub("hello", 2.5, Some(4.9)), "ell");
        assert_eq!(sub("hello", 1.0, Some(1e10)), "");
    }

    #[test]
    fn tostring_glue() {
        assert_eq!(tostring(&3.0), "3");
        assert_eq!(tostring(&0.1), "0.1");
        assert_eq!(tostring("x"), "x");
        assert_eq!(tostring(&Some(1.5)), "1.5");
        assert_eq!(tostring(&None::<f64>), "nil");
        assert_eq!(tostring(&true), "true");
        assert_eq!(crate::lcat!("a", 1.0, "b", 2.5), "a1b2.5");
    }

    #[test]
    fn format_num_sep_defaults() {
        let f = |s: &str| format_num_sep(s, true, ",", ".").0;
        assert_eq!(f("1234.56"), "1,234.56");
        assert_eq!(f("123456"), "123,456");
        assert_eq!(f("-1234567"), "-1,234,567");
        assert_eq!(
            f("^71234 and ^x33FF771234567.5"),
            "^71,234 and ^x33FF771,234,567.5"
        );
        assert_eq!(format_num_sep("1234.5", false, ",", ".").0, "1234.5");
        assert_eq!(
            format_num_sep("1234567.5", true, "\u{a0}", ",").0,
            "1\u{a0}234\u{a0}567,5"
        );
    }
}
