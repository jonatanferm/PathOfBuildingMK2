//! Port of luautf8 `lutf8lib.c` (starwing/luautf8 bdd3d7f, MIT licence, Copyright (c) 2018 Xavier
//! Wang): the `lua-utf8` module PoB requires (`EditControl.lua:10`, `Common.lua:30`). PoB's
//! `runtime/lua-utf8.dll` is built by PathOfBuilding-SimpleGraphic, whose pin `3b1a346` vendors
//! `libs/luautf8` at bdd3d7f (verified: the DLL's export strings list `grapheme_indices` and no
//! `widthlimit`, which is that commit's entry table). Owner: U5 (ui doc §12.3).
//!
//! Only the functions PoB calls are ported (`rg 'utf8\.' src`): `utf8.sub` (Common.lua:820),
//! `utf8.reverse` (812-824), `utf8.find` (818), `utf8.gsub` (813, 824, 826), `utf8.match`
//! (EditControl.lua:555-556, 569-570) and `utf8.next` (EditControl.lua:558, 572). The pattern
//! matcher (`match` and helpers) is shared by find/match/gsub and is ported whole.
//!
//! Semantics that differ from Lua's `string` library and that the port keeps:
//! * strings are decoded as UTF-8 code points; positions passed in and returned by `sub`,
//!   `find` and position captures are **character** indices; `next` takes and returns **byte**
//!   positions;
//! * pattern classes (`%a %c %d %g %l %p %s %t %u %w %x`, upper case negates) use the Unicode
//!   tables of `unidata.h` ([`unidata`]); `%t` is "compose" (combining marks);
//! * an invalid UTF-8 sequence met while decoding the subject, the pattern or a replacement
//!   raises `invalid UTF-8 code` (`utf8_safe_decode`);
//! * in a replacement string `%` must be followed by a digit or `%` (`invalid use of '%' in
//!   replacement string`), and the digit test is Unicode (`utf8_isdigit`);
//! * `utf8.reverse(s, lax)`: without `lax`, combining marks stay attached to their base.
//!
//! Strings are byte slices (`&[u8]`), because Lua strings are bytes: EditControl calls these
//! with `self.buf:sub(...)`, which can cut a multi-byte character, and luautf8 then raises the
//! error above. Integer arguments arrive as Lua numbers (`f64`) and go through
//! `luaL_checkinteger`/`luaL_optinteger`, i.e. `lua_tointeger`, which on 64-bit LuaJIT is the C
//! cast `(lua_Integer)n` (x64 `cvttsd2si`: NaN and out-of-range give `i64::MIN`, ui doc §8 rule 1,
//! CCR-0001), and is then narrowed with `CAST(int, ..)` where the C does so.
//!
//! DIVERGENCE(out-of-buffer reads): the C reads the string's NUL terminator one past the end
//! (`*p` at `p_end`, `iscont(e)`), which [`at`] reproduces (0). A few paths can read further
//! outside the string (`utf8.next(s, 0)` reads the byte before `s`; positions beyond `#s + 1`
//! read past the terminator); those reads are undefined in C and return 0 here.
//!
//! Errors are `lua_error!` panics with luautf8's messages (`luaL_error` from a C function adds no
//! position prefix). Argument-type errors cannot occur: the Rust signatures fix the types.

// A faithful port of C keeps its variable names and control flow (`do { } while (c)`,
// `switch` with one case + default), which these lints flag.
#![allow(
    clippy::many_single_char_names,
    clippy::nonminimal_bool,
    clippy::single_match_else
)]

use std::borrow::Cow;

use pob_lua::pattern::{Capture, Captures};

mod unidata;

// ---------------------------------------------------------------------------------------------
// Byte access with the C string's NUL terminator.
// ---------------------------------------------------------------------------------------------

/// `*p` for a position that may be the NUL terminator (or, divergence above, outside the string).
#[inline]
fn at(s: &[u8], i: isize) -> u8 {
    if i >= 0 && (i as usize) < s.len() {
        s[i as usize]
    } else {
        0
    }
}

/// `iscont(p)`: `((*(p) & 0xC0) == 0x80)`.
/// c: lutf8lib.c:19
#[inline]
fn iscont(s: &[u8], p: isize) -> bool {
    (at(s, p) & 0xC0) == 0x80
}

/// `(lua_Integer)n` (`lua_tointeger` on 64-bit LuaJIT; x64 `cvttsd2si`).
#[inline]
fn tointeger(n: f64) -> i64 {
    super::lua_integer(n)
}

const UTF8_BUFFSZ: usize = 8;
const UTF8_MAX: u32 = 0x7FFF_FFFF;
const UTF8_MAXCP: u32 = 0x10_FFFF;

// ---------------------------------------------------------------------------------------------
// UTF-8 primitives
// ---------------------------------------------------------------------------------------------

/// c: lutf8lib.c:26-27 utf8_invalid
fn utf8_invalid(ch: u32) -> bool {
    ch > UTF8_MAXCP || (0xD800..=0xDFFF).contains(&ch)
}

/// c: lutf8lib.c:29-44 utf8_encode (the bytes `buff[UTF8_BUFFSZ - n ..]`)
fn utf8_encode(out: &mut Vec<u8>, mut x: u32) {
    let mut buff = [0u8; UTF8_BUFFSZ];
    let mut n = 1usize; // number of bytes put in buffer (backwards)
    debug_assert!(x <= UTF8_MAX);
    if x < 0x80 {
        // ascii?
        buff[UTF8_BUFFSZ - 1] = (x & 0x7F) as u8;
    } else {
        // need continuation bytes
        let mut mfb: u32 = 0x3f; // maximum that fits in first byte
        loop {
            // add continuation bytes
            buff[UTF8_BUFFSZ - n] = (0x80 | (x & 0x3f)) as u8;
            n += 1;
            x >>= 6; // remove added bits
            mfb >>= 1; // now there is one less bit available in first byte
            if x <= mfb {
                break; // still needs continuation byte?
            }
        }
        buff[UTF8_BUFFSZ - n] = (((!mfb) << 1) | x) as u8; // add first byte
    }
    out.extend_from_slice(&buff[UTF8_BUFFSZ - n..]);
}

/// `utf8_decode(s + i, &val, strict)`: the position after the sequence and its value, or `None`
/// (NULL) for an invalid sequence.
/// c: lutf8lib.c:46-73 utf8_decode
fn utf8_decode(s: &[u8], i: isize, strict: bool) -> Option<(isize, u32)> {
    const LIMITS: [u32; 6] = [!0u32, 0x80, 0x800, 0x10000, 0x20_0000, 0x400_0000];
    let mut c = u32::from(at(s, i));
    let mut res: u32 = 0; // final result
    let mut i = i;
    if c < 0x80 {
        // ascii?
        res = c;
    } else {
        let mut count: u32 = 0; // to count number of continuation bytes
        while c & 0x40 != 0 {
            // while it needs continuation bytes...
            count += 1;
            let cc = u32::from(at(s, i + count as isize)); // read next byte
            if (cc & 0xC0) != 0x80 {
                // not a continuation byte?
                return None; // invalid byte sequence
            }
            res = (res << 6) | (cc & 0x3F); // add lower 6 bits from cont. byte
            c <<= 1;
        }
        res |= (c & 0x7F).wrapping_shl(count * 5); // add first byte
        if count > 5 || res > UTF8_MAX || res < LIMITS[count as usize] {
            return None; // invalid byte sequence
        }
        i += count as isize; // skip continuation bytes read
    }
    if strict {
        // check for invalid code points; too large or surrogates
        if res > UTF8_MAXCP || (0xD800..=0xDFFF).contains(&res) {
            return None;
        }
    }
    Some((i + 1, res)) // +1 to include first byte
}

/// c: lutf8lib.c:75-78 utf8_prev
fn utf8_prev(src: &[u8], s: isize, mut e: isize) -> isize {
    while s < e && iscont(src, e - 1) {
        e -= 1;
    }
    if s < e {
        e - 1
    } else {
        s
    }
}

/// c: lutf8lib.c:80-83 utf8_next
fn utf8_next(src: &[u8], mut s: isize, e: isize) -> isize {
    while s < e && iscont(src, s + 1) {
        s += 1;
    }
    if s < e {
        s + 1
    } else {
        e
    }
}

/// c: lutf8lib.c:85-90 utf8_length
fn utf8_length(src: &[u8], mut s: isize, e: isize) -> i64 {
    let mut i = 0i64;
    while s < e {
        s = utf8_next(src, s, e);
        i += 1;
    }
    i
}

/// c: lutf8lib.c:92-103 utf8_offset
fn utf8_offset(src: &[u8], s: isize, e: isize, offset: i64, mut idx: i64) -> Option<isize> {
    let mut p = s + offset as isize - 1;
    if idx >= 0 {
        while p < e && idx > 0 {
            p = utf8_next(src, p, e);
            idx -= 1;
        }
        (idx == 0).then_some(p)
    } else {
        while s < p && idx < 0 {
            p = utf8_prev(src, s, p);
            idx += 1;
        }
        (idx == 0).then_some(p)
    }
}

/// c: lutf8lib.c:105-109 utf8_relat
fn utf8_relat(src: &[u8], s: isize, e: isize, idx: i32) -> Option<isize> {
    if idx >= 0 {
        utf8_offset(src, s, e, 1, i64::from(idx) - 1)
    } else {
        utf8_offset(src, s, e, (e - s + 1) as i64, i64::from(idx))
    }
}

/// c: lutf8lib.c:111-117 utf8_range
fn utf8_range(src: &[u8], i: &mut i64, j: &mut i64) -> bool {
    let (s, e) = (0isize, src.len() as isize);
    let ps = utf8_relat(src, s, e, *i as i32); // CAST(int, *i)
    let pe = utf8_relat(src, s, e, *j as i32); // CAST(int, *j)
    *i = (match ps {
        Some(ps) => ps,
        None => {
            if *i > 0 {
                e
            } else {
                s
            }
        }
    } - s) as i64;
    *j = (match pe {
        Some(pe) => utf8_next(src, pe, e),
        None => {
            if *j > 0 {
                e
            } else {
                s
            }
        }
    } - s) as i64;
    *i < *j
}

// ---------------------------------------------------------------------------------------------
// Unicode character categories (unidata.h tables)
// ---------------------------------------------------------------------------------------------

/// c: lutf8lib.c:189-206 find_in_range
fn find_in_range(t: &[(u32, u32, u32)], ch: u32) -> bool {
    let mut begin = 0usize;
    let mut end = t.len();
    while begin < end {
        let mid = (begin + end) / 2;
        if t[mid].1 < ch {
            begin = mid + 1;
        } else if t[mid].0 > ch {
            end = mid;
        } else {
            return (ch - t[mid].0) % t[mid].2 == 0;
        }
    }
    false
}

/// c: lutf8lib.c:208-229 convert_char
fn convert_char(t: &[(u32, u32, u32, i32)], ch: u32) -> u32 {
    let mut begin = 0usize;
    let mut end = t.len();
    while begin < end {
        let mid = (begin + end) / 2;
        if t[mid].1 < ch {
            begin = mid + 1;
        } else if t[mid].0 > ch {
            end = mid;
        } else if (ch - t[mid].0) % t[mid].2 == 0 {
            return ch.wrapping_add_signed(t[mid].3);
        } else {
            return ch;
        }
    }
    ch
}

// c: lutf8lib.c:661-668 define_category / define_converter (utf8_categories, utf8_converters)
fn utf8_isalpha(ch: u32) -> bool {
    find_in_range(unidata::ALPHA, ch)
}
fn utf8_iscntrl(ch: u32) -> bool {
    find_in_range(unidata::CNTRL, ch)
}
fn utf8_isdigit(ch: u32) -> bool {
    find_in_range(unidata::DIGIT, ch)
}
fn utf8_islower(ch: u32) -> bool {
    find_in_range(unidata::LOWER, ch)
}
fn utf8_ispunct(ch: u32) -> bool {
    find_in_range(unidata::PUNCT, ch)
}
fn utf8_isspace(ch: u32) -> bool {
    find_in_range(unidata::SPACE, ch)
}
fn utf8_iscompose(ch: u32) -> bool {
    find_in_range(unidata::COMPOSE, ch)
}
fn utf8_isupper(ch: u32) -> bool {
    find_in_range(unidata::UPPER, ch)
}
fn utf8_isxdigit(ch: u32) -> bool {
    find_in_range(unidata::XDIGIT, ch)
}
fn utf8_tolower(ch: u32) -> u32 {
    convert_char(unidata::TOLOWER, ch)
}

/// c: lutf8lib.c:670-678 utf8_isgraph
fn utf8_isgraph(ch: u32) -> bool {
    if find_in_range(unidata::SPACE, ch) {
        return false;
    }
    if find_in_range(unidata::GRAPH, ch) {
        return true;
    }
    if find_in_range(unidata::COMPOSE, ch) {
        return true;
    }
    false
}

/// c: lutf8lib.c:680-686 utf8_isalnum
fn utf8_isalnum(ch: u32) -> bool {
    if find_in_range(unidata::ALPHA, ch) {
        return true;
    }
    if find_in_range(unidata::ALNUM_EXTEND, ch) {
        return true;
    }
    false
}

/// `utf8_safe_decode`: decode or raise `invalid UTF-8 code`.
/// c: lutf8lib.c:719-723 utf8_safe_decode
fn utf8_safe_decode(s: &[u8], p: isize) -> (isize, u32) {
    match utf8_decode(s, p, false) {
        Some(r) => r,
        None => pob_lua::lua_error!("invalid UTF-8 code"),
    }
}

/// c: lutf8lib.c:731-735 byte_relat
fn byte_relat(pos: i64, len: usize) -> i64 {
    if pos >= 0 {
        pos
    } else if 0usize.wrapping_sub(pos as usize) > len {
        0
    } else {
        len as i64 + pos + 1
    }
}

// ---------------------------------------------------------------------------------------------
// utf8.sub / utf8.reverse / utf8.next
// ---------------------------------------------------------------------------------------------

/// `utf8.sub(s, i [, j])`: characters `i..j` (1-based, negative from the end).
/// c: lutf8lib.c:765-774 Lutf8_sub
pub fn sub(s: &[u8], i: f64, j: Option<f64>) -> &[u8] {
    let mut posi = tointeger(i);
    let mut pose = j.map_or(-1, tointeger);
    if utf8_range(s, &mut posi, &mut pose) {
        &s[posi as usize..pose as usize]
    } else {
        b""
    }
}

/// `utf8.reverse(s [, lax])`. `lax` is the Lua truthiness of the second argument: note that
/// `utf8.reverse(utf8.gsub(..))` passes gsub's count, a number, so it is lax (Common.lua:813).
/// c: lutf8lib.c:776-802 Lutf8_reverse
pub fn reverse(s: &[u8], lax: bool) -> Vec<u8> {
    let start = 0isize;
    let mut e = s.len() as isize;
    let mut b: Vec<u8> = Vec::with_capacity(s.len());
    if lax {
        let mut prev = e;
        while start < prev {
            prev = utf8_prev(s, start, prev);
            b.extend_from_slice(&s[prev as usize..e as usize]);
            e = prev;
        }
    } else {
        let mut prev = e;
        while start < prev {
            let pprev = utf8_prev(s, start, prev);
            // `assert(ends == prev)` is compiled out (the DLL is a Release build): a stray
            // continuation byte after a complete sequence is copied along with it.
            let (_ends, code) = utf8_safe_decode(s, pprev);
            if utf8_invalid(code) {
                pob_lua::lua_error!("invalid UTF-8 code");
            }
            if !utf8_iscompose(code) {
                b.extend_from_slice(&s[pprev as usize..e as usize]);
                e = pprev;
            }
            prev = pprev;
        }
    }
    b
}

/// c: lutf8lib.c:972-984 push_offset (`None` = no values)
fn push_offset(src: &[u8], offset: i64, idx: i64) -> Option<(f64, f64)> {
    let (s, e) = (0isize, src.len() as isize);
    let p = if idx != 0 {
        utf8_offset(src, s, e, offset, idx)
    } else {
        let p = s + offset as isize - 1;
        if iscont(src, p) {
            Some(utf8_prev(src, s, p))
        } else {
            Some(p)
        }
    };
    let p = p?;
    if p == e {
        return None;
    }
    // utf8_decode(p, &ch, 0): ch stays 0 for an invalid sequence
    let ch = utf8_decode(src, p, false).map_or(0, |(_, ch)| ch);
    Some(((p - s + 1) as f64, f64::from(ch)))
}

/// `utf8.next(s [, i [, off]])`: the byte position and code point of the character `off`
/// characters after (negative: before) byte position `i`; `None` is Lua's empty return (nil in
/// `utf8.next(..) or x`). `i` omitted (nil) means 1 with `off` defaulting to 0; `i` given means
/// `off` defaults to 1.
/// c: lutf8lib.c:1038-1043 Lutf8_next
pub fn next(s: &[u8], i: Option<f64>, off: Option<f64>) -> Option<(f64, f64)> {
    let offset = byte_relat(i.map_or(1, tointeger), s.len());
    let idx = off.map_or(i64::from(i.is_some()), tointeger);
    push_offset(s, offset, idx)
}

// ---------------------------------------------------------------------------------------------
// Pattern matching
// ---------------------------------------------------------------------------------------------

const LUA_MAXCAPTURES: usize = 32;
const CAP_UNFINISHED: isize = -1;
const CAP_POSITION: isize = -2;
const MAXCCALLS: i32 = 200;
const L_ESC: u32 = b'%' as u32;
const SPECIALS: &[u8] = b"^$*+?.([%-";

/// c: lutf8lib.c:1158-1169 MatchState
struct MatchState<'s, 'p> {
    matchdepth: i32,
    src: &'s [u8],
    pat: &'p [u8],
    level: usize,
    capture: [(isize, isize); LUA_MAXCAPTURES],
}

impl MatchState<'_, '_> {
    fn src_end(&self) -> isize {
        self.src.len() as isize
    }
    fn p_end(&self) -> isize {
        self.pat.len() as isize
    }
    fn pb(&self, p: isize) -> u8 {
        at(self.pat, p)
    }
}

/// c: lutf8lib.c:1182-1187 check_capture
fn check_capture(ms: &MatchState<'_, '_>, l: u32) -> usize {
    let l = l as i64 - i64::from(b'1');
    if l < 0 || l as usize >= ms.level || ms.capture[l as usize].1 == CAP_UNFINISHED {
        pob_lua::lua_error!("invalid capture index %{}", l + 1);
    }
    l as usize
}

/// c: lutf8lib.c:1189-1194 capture_to_close
fn capture_to_close(ms: &MatchState<'_, '_>) -> usize {
    let mut level = ms.level as isize;
    loop {
        level -= 1;
        if level < 0 {
            break;
        }
        if ms.capture[level as usize].1 == CAP_UNFINISHED {
            return level as usize;
        }
    }
    pob_lua::lua_error!("invalid pattern capture")
}

/// c: lutf8lib.c:1196-1219 classend
fn classend(ms: &MatchState<'_, '_>, p: isize) -> isize {
    let (mut p, ch) = utf8_safe_decode(ms.pat, p);
    match ch {
        L_ESC => {
            if p == ms.p_end() {
                pob_lua::lua_error!("malformed pattern (ends with '%')");
            }
            utf8_next(ms.pat, p, ms.p_end())
        }
        0x5B /* '[' */ => {
            if ms.pb(p) == b'^' {
                p += 1;
            }
            loop {
                // look for a `]'
                if p == ms.p_end() {
                    pob_lua::lua_error!("malformed pattern (missing ']')");
                }
                let c = ms.pb(p);
                p += 1;
                if u32::from(c) == L_ESC && p < ms.p_end() {
                    p += 1; // skip escapes (e.g. `%]')
                }
                if ms.pb(p) == b']' {
                    break;
                }
            }
            p + 1
        }
        _ => p,
    }
}

/// c: lutf8lib.c:1221-1233 match_class
fn match_class(c: u32, cl: u32) -> bool {
    let res = match utf8_tolower(cl) {
        0x61 /* 'a' */ => utf8_isalpha(c),
        0x63 /* 'c' */ => utf8_iscntrl(c),
        0x64 /* 'd' */ => utf8_isdigit(c),
        0x6C /* 'l' */ => utf8_islower(c),
        0x70 /* 'p' */ => utf8_ispunct(c),
        0x73 /* 's' */ => utf8_isspace(c),
        0x74 /* 't' */ => utf8_iscompose(c),
        0x75 /* 'u' */ => utf8_isupper(c),
        0x78 /* 'x' */ => utf8_isxdigit(c),
        0x67 /* 'g' */ => utf8_isgraph(c),
        0x77 /* 'w' */ => utf8_isalnum(c),
        0x7A /* 'z' */ => c == 0, // deprecated option
        _ => return cl == c,
    };
    if utf8_islower(cl) {
        res
    } else {
        !res
    }
}

/// c: lutf8lib.c:1235-1261 matchbracketclass
fn matchbracketclass(ms: &MatchState<'_, '_>, c: u32, p: isize, ec: isize) -> bool {
    let mut sig = true;
    debug_assert_eq!(ms.pb(p), b'[');
    let mut p = p + 1;
    if ms.pb(p) == b'^' {
        sig = false;
        p += 1; // skip the `^'
    }
    while p < ec {
        let (np, ch) = utf8_safe_decode(ms.pat, p);
        p = np;
        if ch == L_ESC {
            let (np, ch) = utf8_safe_decode(ms.pat, p);
            p = np;
            if match_class(c, ch) {
                return sig;
            }
        } else {
            let (np, next) = utf8_safe_decode(ms.pat, p);
            if next == u32::from(b'-') && np < ec {
                let (np2, next) = utf8_safe_decode(ms.pat, np);
                p = np2;
                if ch <= c && c <= next {
                    return sig;
                }
            } else if ch == c {
                return sig;
            }
        }
    }
    !sig
}

/// c: lutf8lib.c:1263-1278 singlematch
fn singlematch(ms: &MatchState<'_, '_>, s: isize, p: isize, ep: isize) -> bool {
    if s >= ms.src_end() {
        false
    } else {
        let (_, ch) = utf8_safe_decode(ms.src, s);
        let (p, pch) = utf8_safe_decode(ms.pat, p);
        match pch {
            0x2E /* '.' */ => true, // matches any char
            L_ESC => {
                let (_, pch) = utf8_safe_decode(ms.pat, p);
                match_class(ch, pch)
            }
            0x5B /* '[' */ => matchbracketclass(ms, ch, p - 1, ep - 1),
            _ => pch == ch,
        }
    }
}

/// c: lutf8lib.c:1280-1300 matchbalance
fn matchbalance(ms: &MatchState<'_, '_>, s: isize, p: &mut isize) -> Option<isize> {
    let (np, begin) = utf8_safe_decode(ms.pat, *p);
    *p = np;
    if *p >= ms.p_end() {
        pob_lua::lua_error!("malformed pattern (missing arguments to '%b')");
    }
    let (np, end) = utf8_safe_decode(ms.pat, *p);
    *p = np;
    let (mut s, ch) = utf8_safe_decode(ms.src, s);
    if ch != begin {
        return None;
    }
    let mut cont = 1;
    while s < ms.src_end() {
        let (ns, ch) = utf8_safe_decode(ms.src, s);
        s = ns;
        if ch == end {
            cont -= 1;
            if cont == 0 {
                return Some(s);
            }
        } else if ch == begin {
            cont += 1;
        }
    }
    None // string ends out of balance
}

/// c: lutf8lib.c:1302-1315 max_expand
fn max_expand(ms: &mut MatchState<'_, '_>, s: isize, p: isize, ep: isize) -> Option<isize> {
    let mut m = s; // matched end of single match p
    while singlematch(ms, m, p, ep) {
        m = utf8_next(ms.src, m, ms.src_end());
    }
    // keeps trying to match with the maximum repetitions
    while s <= m {
        if let Some(res) = do_match(ms, m, ep + 1) {
            return Some(res);
        }
        // else didn't match; reduce 1 repetition to try again
        if s == m {
            break;
        }
        m = utf8_prev(ms.src, s, m);
    }
    None
}

/// c: lutf8lib.c:1317-1326 min_expand
fn min_expand(ms: &mut MatchState<'_, '_>, mut s: isize, p: isize, ep: isize) -> Option<isize> {
    loop {
        if let Some(res) = do_match(ms, s, ep + 1) {
            return Some(res);
        } else if singlematch(ms, s, p, ep) {
            s = utf8_next(ms.src, s, ms.src_end()); // try with one more repetition
        } else {
            return None;
        }
    }
}

/// c: lutf8lib.c:1328-1338 start_capture
fn start_capture(ms: &mut MatchState<'_, '_>, s: isize, p: isize, what: isize) -> Option<isize> {
    let level = ms.level;
    if level >= LUA_MAXCAPTURES {
        pob_lua::lua_error!("too many captures");
    }
    ms.capture[level] = (s, what);
    ms.level = level + 1;
    let res = do_match(ms, s, p);
    if res.is_none() {
        ms.level -= 1; // undo capture
    }
    res
}

/// c: lutf8lib.c:1340-1347 end_capture
fn end_capture(ms: &mut MatchState<'_, '_>, s: isize, p: isize) -> Option<isize> {
    let l = capture_to_close(ms);
    ms.capture[l].1 = s - ms.capture[l].0; // close capture
    let res = do_match(ms, s, p);
    if res.is_none() {
        ms.capture[l].1 = CAP_UNFINISHED; // undo capture
    }
    res
}

/// c: lutf8lib.c:1349-1357 match_capture
fn match_capture(ms: &MatchState<'_, '_>, s: isize, l: u32) -> Option<isize> {
    let l = check_capture(ms, l);
    let (init, len) = ms.capture[l];
    // `len` is a size_t: CAP_POSITION (-2) is huge and never fits.
    if len < 0 || ms.src_end() - s < len {
        return None;
    }
    let (a, b) = (init as usize, (init + len) as usize);
    let (c, d) = (s as usize, (s + len) as usize);
    (ms.src[a..b] == ms.src[c..d]).then_some(s + len)
}

/// `match(ms, s, p)`: the end of the match of pattern position `p` at subject position `s`.
/// c: lutf8lib.c:1359-1463 match
fn do_match(ms: &mut MatchState<'_, '_>, s: isize, p: isize) -> Option<isize> {
    if ms.matchdepth == 0 {
        pob_lua::lua_error!("pattern too complex");
    }
    ms.matchdepth -= 1;
    let mut s = Some(s);
    let mut p = p;
    // init: (using goto's to optimize tail recursion)
    'init: loop {
        let Some(si) = s else { break 'init };
        if p == ms.p_end() {
            break 'init; // end of pattern
        }
        let (_, ch) = utf8_safe_decode(ms.pat, p);
        let mut dflt = false;
        match ch {
            0x28 /* '(' */ => {
                // start capture
                s = if ms.pb(p + 1) == b')' {
                    // position capture?
                    start_capture(ms, si, p + 2, CAP_POSITION)
                } else {
                    start_capture(ms, si, p + 1, CAP_UNFINISHED)
                };
            }
            0x29 /* ')' */ => {
                // end capture
                s = end_capture(ms, si, p + 1);
            }
            0x24 /* '$' */ => {
                if p + 1 != ms.p_end() {
                    // is the `$' the last char in pattern?
                    dflt = true; // no; go to default
                } else {
                    s = (si == ms.src_end()).then_some(si); // check end of string
                }
            }
            L_ESC => {
                // escaped sequence not in the format class[*+?-]?
                let prev_p = p;
                let (np, ch) = utf8_safe_decode(ms.pat, p + 1);
                p = np;
                match ch {
                    0x62 /* 'b' */ => {
                        // balanced string?
                        s = matchbalance(ms, si, &mut p);
                        if s.is_some() {
                            continue 'init; // return match(ms, s, p + 4);
                        }
                        // else fail (s == NULL)
                    }
                    0x66 /* 'f' */ => {
                        // frontier?
                        if ms.pb(p) != b'[' {
                            pob_lua::lua_error!("missing '[' after '%f' in pattern");
                        }
                        let ep = classend(ms, p); // points to what is next
                        let mut previous = 0u32;
                        let mut current = 0u32;
                        if si != 0 {
                            if let Some((_, v)) =
                                utf8_decode(ms.src, utf8_prev(ms.src, 0, si), false)
                            {
                                previous = v;
                            }
                        }
                        if si != ms.src_end() {
                            if let Some((_, v)) = utf8_decode(ms.src, si, false) {
                                current = v;
                            }
                        }
                        if !matchbracketclass(ms, previous, p, ep - 1)
                            && matchbracketclass(ms, current, p, ep - 1)
                        {
                            p = ep;
                            continue 'init; // return match(ms, s, ep);
                        }
                        s = None; // match failed
                    }
                    0x30..=0x39 => {
                        // capture results (%0-%9)?
                        s = match_capture(ms, si, ch);
                        if s.is_some() {
                            continue 'init; // return match(ms, s, p + 2)
                        }
                    }
                    _ => {
                        p = prev_p;
                        dflt = true;
                    }
                }
            }
            _ => dflt = true,
        }
        if !dflt {
            break 'init;
        }
        // default: dflt: pattern class plus optional suffix
        let ep = classend(ms, p); // points to optional suffix
        let epc = ms.pb(ep);
        if !singlematch(ms, si, p, ep) {
            // does not match at least once?
            if epc == b'*' || epc == b'?' || epc == b'-' {
                // accept empty?
                p = ep + 1;
                continue 'init; // return match(ms, s, ep + 1);
            }
            // '+' or no suffix
            s = None; // fail
        } else {
            // matched once
            let next_s = utf8_next(ms.src, si, ms.src_end());
            match epc {
                b'?' => {
                    // optional
                    let next_ep = utf8_next(ms.pat, ep, ms.p_end());
                    if let Some(res) = do_match(ms, next_s, next_ep) {
                        s = Some(res);
                    } else {
                        p = next_ep;
                        continue 'init; // else return match(ms, s, ep + 1);
                    }
                }
                b'+' => {
                    // 1 or more repetitions
                    s = max_expand(ms, next_s, p, ep); // 1 match already done
                }
                b'*' => {
                    // 0 or more repetitions
                    s = max_expand(ms, si, p, ep);
                }
                b'-' => {
                    // 0 or more repetitions (minimum)
                    s = min_expand(ms, si, p, ep);
                }
                _ => {
                    // no suffix
                    s = Some(next_s);
                    p = ep;
                    continue 'init; // return match(ms, s + 1, ep);
                }
            }
        }
        break 'init;
    }
    ms.matchdepth += 1;
    s
}

/// c: lutf8lib.c:1465-1483 lmemfind
fn lmemfind(s1: &[u8], s2: &[u8]) -> Option<usize> {
    if s2.is_empty() {
        return Some(0); // empty strings are everywhere
    }
    if s2.len() > s1.len() {
        return None; // avoids a negative `l1'
    }
    s1.windows(s2.len()).position(|w| w == s2)
}

/// c: lutf8lib.c:1485-1490 get_index
fn get_index(src: &[u8], p: isize, mut s: isize, e: isize) -> i64 {
    let mut idx = 0i64;
    while s < e && s < p {
        s = utf8_next(src, s, e);
        idx += 1;
    }
    if s == p {
        idx
    } else {
        idx - 1
    }
}

/// c: lutf8lib.c:1492-1507 push_onecapture
fn push_onecapture<'s>(ms: &MatchState<'s, '_>, i: usize, s: isize, e: isize) -> Capture<'s> {
    if i >= ms.level {
        if i == 0 {
            // ms->level == 0, too
            Capture::Str(&ms.src[s as usize..e as usize]) // add whole match
        } else {
            pob_lua::lua_error!("invalid capture index")
        }
    } else {
        let (init, l) = ms.capture[i];
        if l == CAP_UNFINISHED {
            pob_lua::lua_error!("unfinished capture");
        }
        if l == CAP_POSITION {
            let idx = get_index(ms.src, init, 0, ms.src_end());
            Capture::Pos((idx + 1) as usize)
        } else {
            Capture::Str(&ms.src[init as usize..(init + l) as usize])
        }
    }
}

/// `push_captures(ms, s, e)`; `whole` is `None` for `find` (`push_captures(&ms, NULL, 0)`).
/// c: lutf8lib.c:1509-1516 push_captures
fn push_captures<'s>(ms: &MatchState<'s, '_>, whole: Option<(isize, isize)>) -> Captures<'s> {
    let nlevels = if ms.level == 0 && whole.is_some() {
        1
    } else {
        ms.level
    };
    let (s, e) = whole.unwrap_or((0, 0));
    (0..nlevels).map(|i| push_onecapture(ms, i, s, e)).collect()
}

/// c: lutf8lib.c:1519-1527 nospecials
fn nospecials(p: &[u8]) -> bool {
    !p.iter().any(|c| SPECIALS.contains(c))
}

/// The results of `utf8.find`: start and end character indices and the captures.
pub type FindResult<'s> = (f64, f64, Captures<'s>);

/// `find_aux(L, find)`: `Ok` = find results, `Err` = match captures (one enum keeps the single
/// C body).
/// c: lutf8lib.c:1531-1585 find_aux
fn find_aux<'s>(
    s: &'s [u8],
    p: &[u8],
    init_arg: Option<f64>,
    plain: bool,
    find: bool,
) -> Option<Result<FindResult<'s>, Captures<'s>>> {
    let es = s.len() as isize;
    let mut idx: i64 = init_arg.map_or(1, tointeger);
    if idx == 0 {
        idx = 1;
    }
    let init = match utf8_relat(s, 0, es, idx as i32) {
        Some(init) => init,
        None => {
            if idx > 0 {
                return None; // cannot find anything
            }
            0
        }
    };
    // explicit request or no special characters?
    if find && (plain || nospecials(p)) {
        // do a plain search
        if let Some(off) = lmemfind(&s[init as usize..], p) {
            let s2 = init + off as isize;
            let mut e2 = s2 + p.len() as isize;
            if iscont(s, e2) {
                e2 = utf8_next(s, e2, es);
            }
            idx = get_index(s, s2, 0, es) + 1;
            let end = idx + get_index(s, e2, s2, es) - 1;
            return Some(Ok((idx as f64, end as f64, Captures::new())));
        }
    } else {
        let anchor = p.first() == Some(&b'^');
        let p = if anchor { &p[1..] } else { p }; // skip anchor character
        if idx < 0 {
            idx += utf8_length(s, 0, es) + 1; // TODO not very good
        }
        let mut init = init;
        loop {
            let mut ms = MatchState {
                matchdepth: MAXCCALLS,
                src: s,
                pat: p,
                level: 0,
                capture: [(0, 0); LUA_MAXCAPTURES],
            };
            if let Some(res) = do_match(&mut ms, init, 0) {
                return Some(if find {
                    let end = idx + utf8_length(s, init, res) - 1;
                    Ok((idx as f64, end as f64, push_captures(&ms, None)))
                } else {
                    Err(push_captures(&ms, Some((init, res))))
                });
            }
            if init == es {
                break;
            }
            idx += 1;
            init = utf8_next(s, init, es);
            if !(init <= es && !anchor) {
                break;
            }
        }
    }
    None // not found
}

/// `utf8.find(s, p [, init [, plain]])`: start and end **character** indices and the captures
/// (position captures are character indices). `plain` is the Lua truthiness of the 4th argument
/// (Common.lua:818 passes `2`).
/// c: lutf8lib.c:1587 Lutf8_find
pub fn find<'s>(s: &'s [u8], p: &[u8], init: Option<f64>, plain: bool) -> Option<FindResult<'s>> {
    match find_aux(s, p, init, plain, true)? {
        Ok(r) => Some(r),
        Err(_) => unreachable!("find_aux(find = 1) returns find results"),
    }
}

/// `utf8.match(s, p [, init])`: the captures (or the whole match).
/// c: lutf8lib.c:1588 Lutf8_match
pub fn r#match<'s>(s: &'s [u8], p: &[u8], init: Option<f64>) -> Option<Captures<'s>> {
    match find_aux(s, p, init, false, false)? {
        Err(c) => Some(c),
        Ok(_) => unreachable!("find_aux(find = 0) returns captures"),
    }
}

/// `utf8.match(s, p)` in expression position (ui doc §8 rule 5): the first value as bytes;
/// a position capture shows as its number (Lua `tostring`), nil is `None`.
pub fn match1(s: &[u8], p: &[u8]) -> Option<Vec<u8>> {
    let caps = r#match(s, p, None)?;
    caps.first().map(|c| match c {
        Capture::Str(b) => b.to_vec(),
        Capture::Pos(n) => pob_lua::tostring(*n as f64).into_bytes(),
    })
}

/// `add_utf8char(b, ch)`.
/// c: lutf8lib.c:725-729 add_utf8char
fn add_utf8char(b: &mut Vec<u8>, ch: u32) {
    utf8_encode(b, ch);
}

/// A capture's text as `luaL_addvalue` adds it (a position capture is a number: `tostring`).
fn push_capture_text(b: &mut Vec<u8>, c: &Capture<'_>) {
    match c {
        Capture::Str(s) => b.extend_from_slice(s),
        Capture::Pos(n) => b.extend_from_slice(pob_lua::tostring(*n as f64).as_bytes()),
    }
}

/// c: lutf8lib.c:1627-1649 add_s
fn add_s(ms: &MatchState<'_, '_>, b: &mut Vec<u8>, s: isize, e: isize, news: &[u8]) {
    let new_end = news.len() as isize;
    let mut i = 0isize;
    while i < new_end {
        let (ni, ch) = utf8_safe_decode(news, i);
        i = ni;
        if ch != L_ESC {
            add_utf8char(b, ch);
        } else {
            let (ni, ch) = utf8_safe_decode(news, i); // skip ESC
            i = ni;
            if !utf8_isdigit(ch) {
                if ch != L_ESC {
                    pob_lua::lua_error!("invalid use of '%' in replacement string");
                }
                add_utf8char(b, ch);
            } else if ch == u32::from(b'0') {
                b.extend_from_slice(&ms.src[s as usize..e as usize]);
            } else {
                let c = push_onecapture(ms, ch.wrapping_sub(u32::from(b'1')) as usize, s, e);
                push_capture_text(b, &c); // add capture to accumulated result
            }
        }
    }
}

/// Replacement argument of [`gsub`] (`lua_type(L, 3)`).
pub enum Repl<'r> {
    /// A string (or a number, already `tostring`-ed by the caller: `lua_tolstring`).
    Str(&'r [u8]),
    /// A function of the captures; `None` = nil/false keeps the original match. Lua numbers
    /// returned by the function must be `tostring`-ed by the caller.
    Func(&'r mut dyn FnMut(&Captures<'_>) -> Option<Vec<u8>>),
}

/// `utf8.gsub(s, p, repl [, n])`: the result and the number of substitutions.
/// c: lutf8lib.c:1651-1720 add_value, Lutf8_gsub
pub fn gsub_with(s: &[u8], p: &[u8], mut repl: Repl<'_>, max_n: Option<f64>) -> (Vec<u8>, f64) {
    let es = s.len() as isize;
    let max_s: i64 = max_n.map_or(s.len() as i64 + 1, tointeger);
    let anchor = p.first() == Some(&b'^');
    let mut n: i64 = 0;
    let mut b: Vec<u8> = Vec::with_capacity(s.len());
    let p = if anchor { &p[1..] } else { p }; // skip anchor character
    let mut si = 0isize;
    while n < max_s {
        let mut ms = MatchState {
            matchdepth: MAXCCALLS,
            src: s,
            pat: p,
            level: 0,
            capture: [(0, 0); LUA_MAXCAPTURES],
        };
        let e = do_match(&mut ms, si, 0);
        if let Some(e) = e {
            n += 1;
            // add_value
            match &mut repl {
                Repl::Str(news) => add_s(&ms, &mut b, si, e, news),
                Repl::Func(f) => {
                    let caps = push_captures(&ms, Some((si, e)));
                    match f(&caps) {
                        Some(v) => b.extend_from_slice(&v),
                        None => b.extend_from_slice(&s[si as usize..e as usize]), // keep original text
                    }
                }
            }
        }
        match e {
            Some(e) if e > si => si = e, // non empty match? skip it
            _ if si < es => {
                let (ns, ch) = utf8_safe_decode(s, si);
                si = ns;
                add_utf8char(&mut b, ch);
            }
            _ => break,
        }
        if anchor {
            break;
        }
    }
    b.extend_from_slice(&s[si as usize..]);
    (b, n as f64)
}

/// `utf8.gsub(s, p, repl [, n])` with a string replacement.
pub fn gsub(s: &[u8], p: &[u8], repl: &[u8], max_n: Option<f64>) -> (Vec<u8>, f64) {
    gsub_with(s, p, Repl::Str(repl), max_n)
}

/// `&str` convenience for results that callers keep as text (lossy only when the byte result
/// is not UTF-8, ui doc §8 rule 4).
pub fn lossy(b: &[u8]) -> Cow<'_, str> {
    String::from_utf8_lossy(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps(s: &str, p: &str) -> Option<Vec<String>> {
        r#match(s.as_bytes(), p.as_bytes(), None)
            .map(|v| v.iter().map(|c| c.to_str().into_owned()).collect())
    }

    #[test]
    fn basics() {
        assert_eq!(sub("aéb€c".as_bytes(), 2.0, Some(4.0)), "éb€".as_bytes());
        assert_eq!(sub("aéb€c".as_bytes(), -2.0, None), "€c".as_bytes());
        assert_eq!(reverse("aéb".as_bytes(), false), "béa".as_bytes());
        // a combining acute accent stays with its base unless lax
        assert_eq!(
            reverse("ae\u{301}b".as_bytes(), false),
            "be\u{301}a".as_bytes()
        );
        assert_eq!(
            reverse("ae\u{301}b".as_bytes(), true),
            "b\u{301}ea".as_bytes()
        );
        assert_eq!(
            next("héllo".as_bytes(), Some(4.0), Some(-1.0)),
            Some((2.0, 233.0))
        );
        assert_eq!(
            next("héllo".as_bytes(), Some(2.0), Some(1.0)),
            Some((4.0, 108.0))
        );
        assert_eq!(next("ab".as_bytes(), Some(2.0), Some(1.0)), None);
        assert_eq!(caps("héllo wörld", "%w*$"), Some(vec!["wörld".into()]));
        assert_eq!(caps("a()b", "()b"), Some(vec!["4".into()]));
        // a stray continuation byte travels with the character before it (utf8_next)
        assert_eq!(caps("a\u{e9}", "%w*$"), Some(vec!["a\u{e9}".into()]));
        let (r, n) = gsub(b"7654321", b"(%d%d%d)", b"%1,", None);
        assert_eq!((r.as_slice(), n), (&b"765,432,1"[..], 2.0));
        let f = find("aéb".as_bytes(), b"b", None, false).unwrap();
        assert_eq!((f.0, f.1), (3.0, 3.0));
    }

    #[test]
    fn errors() {
        assert_eq!(
            r#match(b"a\x80", b"%w*$", None).unwrap()[0].as_bytes(),
            b"a\x80"
        );
        let e = pob_lua::error::catch(|| r#match(b"a\xC3", b"%w*$", None)).unwrap_err();
        assert_eq!(e.0, "invalid UTF-8 code");
        let e = pob_lua::error::catch(|| gsub(b"abc", b"b", b"%x", None)).unwrap_err();
        assert_eq!(e.0, "invalid use of '%' in replacement string");
    }
}
