//! Lua 5.1 string patterns as implemented by LuaJIT 2.1 `lib_string.c`
//! (`string.find`, `string.match`, `string.gmatch`, `string.gsub`).
//!
//! Moved here from `crates/pob-calc/src/lua_pattern.rs` (AMENDMENTS A1) and
//! brought in line with LuaJIT's `lib_string.c` rather than PUC Lua:
//! * patterns and subjects are NUL-terminated C strings (`*p == '\0'` ends a
//!   pattern; reading one past the subject yields `'\0'`), emulated by
//!   [`at`];
//! * character classes use LuaJIT's ASCII `lj_char` table (bytes 128-255 are
//!   in no class) and include `%g`;
//! * `find`/`match` clamp `init` past the end to the end (LuaJIT without
//!   `LJ_52`), instead of returning nil;
//! * the matcher recursion limit is `LJ_MAX_XLEVEL` (200): "pattern too
//!   complex";
//! * errors are raised with `lua_error!` using LuaJIT's messages.
//!
//! The byte-level functions (`*_bytes`) are the reference; the `&str`
//! functions wrap them, re-validating results lossily when a byte-level
//! operation split a UTF-8 character (ui doc §8 rule 4).

use std::borrow::Cow;

use smallvec::SmallVec;

use crate::format::{at, lj_char_isa, lj_char_isdigit, tonumber_bytes, tostring_num};
use crate::format::{
    LJ_CHAR_ALNUM, LJ_CHAR_ALPHA, LJ_CHAR_CNTRL, LJ_CHAR_DIGIT, LJ_CHAR_GRAPH, LJ_CHAR_LOWER,
    LJ_CHAR_PUNCT, LJ_CHAR_SPACE, LJ_CHAR_UPPER, LJ_CHAR_XDIGIT,
};

const LUA_MAXCAPTURES: usize = 32;
const LJ_MAX_XLEVEL: u32 = 200;
const L_ESC: u8 = b'%';
const CAP_UNFINISHED: isize = -1;
const CAP_POSITION: isize = -2;

/// A capture result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capture<'s> {
    Str(&'s [u8]),
    /// `()` position capture: 1-based byte index.
    Pos(usize),
}

/// The captures of one match (`string.match` results).
pub type Captures<'s> = SmallVec<[Capture<'s>; 4]>;

impl<'s> Capture<'s> {
    /// The capture's bytes (`""` for a position capture).
    pub fn as_bytes(&self) -> &'s [u8] {
        match self {
            Capture::Str(s) => s,
            Capture::Pos(_) => b"",
        }
    }

    /// The capture as `&str`. Panics if a byte-level match split a UTF-8
    /// character (use [`Capture::to_str`] where that can happen).
    pub fn as_str(&self) -> &'s str {
        match self {
            Capture::Str(s) => std::str::from_utf8(s).unwrap_or_else(|_| {
                panic!("pob_lua::pattern: capture is not UTF-8 (use Capture::to_str)")
            }),
            Capture::Pos(_) => "",
        }
    }

    /// The capture as text the way Lua's `tostring` would show it: position
    /// captures become their number; invalid UTF-8 is replaced lossily.
    pub fn to_str(&self) -> Cow<'s, str> {
        match self {
            Capture::Str(s) => String::from_utf8_lossy(s),
            Capture::Pos(p) => Cow::Owned(tostring_num(*p as f64)),
        }
    }

    /// Lua `tonumber(cap)`.
    pub fn num(&self) -> Option<f64> {
        match self {
            Capture::Str(s) => tonumber_bytes(s),
            Capture::Pos(p) => Some(*p as f64),
        }
    }

    /// Lua's `..`/`tostring` text of the capture as bytes.
    fn push_to(&self, out: &mut Vec<u8>) {
        match self {
            Capture::Str(s) => out.extend_from_slice(s),
            Capture::Pos(p) => out.extend_from_slice(tostring_num(*p as f64).as_bytes()),
        }
    }
}

struct MatchState<'s, 'p> {
    src: &'s [u8],
    pat: &'p [u8],
    level: usize,
    depth: u32,
    capture: [(usize, isize); LUA_MAXCAPTURES],
}

impl<'s, 'p> MatchState<'s, 'p> {
    fn new(src: &'s [u8], pat: &'p [u8]) -> Self {
        Self {
            src,
            pat,
            level: 0,
            depth: 0,
            capture: [(0, 0); LUA_MAXCAPTURES],
        }
    }
    #[inline]
    fn p(&self, i: usize) -> u8 {
        at(self.pat, i)
    }
    #[inline]
    fn s(&self, i: usize) -> u8 {
        at(self.src, i)
    }
    #[inline]
    fn src_end(&self) -> usize {
        self.src.len()
    }
}

fn check_capture(ms: &MatchState<'_, '_>, l: u8) -> usize {
    let l = i32::from(l) - i32::from(b'1');
    if l < 0 || l as usize >= ms.level || ms.capture[l as usize].1 == CAP_UNFINISHED {
        crate::lua_error!("invalid capture index");
    }
    l as usize
}

fn capture_to_close(ms: &MatchState<'_, '_>) -> usize {
    let mut level = ms.level as isize - 1;
    while level >= 0 {
        if ms.capture[level as usize].1 == CAP_UNFINISHED {
            return level as usize;
        }
        level -= 1;
    }
    crate::lua_error!("invalid pattern capture");
}

fn classend(ms: &MatchState<'_, '_>, mut p: usize) -> usize {
    let c = ms.p(p);
    p += 1;
    match c {
        L_ESC => {
            if ms.p(p) == 0 {
                crate::lua_error!("malformed pattern (ends with '%')");
            }
            p + 1
        }
        b'[' => {
            if ms.p(p) == b'^' {
                p += 1;
            }
            loop {
                // look for a `]'
                if ms.p(p) == 0 {
                    crate::lua_error!("malformed pattern (missing ']')");
                }
                let c = ms.p(p);
                p += 1;
                if c == L_ESC && ms.p(p) != 0 {
                    p += 1; // skip escapes (e.g. `%]')
                }
                if ms.p(p) == b']' {
                    break;
                }
            }
            p + 1
        }
        _ => p,
    }
}

#[rustfmt::skip]
static MATCH_CLASS_MAP: [u8; 32] = [
  0,LJ_CHAR_ALPHA,0,LJ_CHAR_CNTRL,LJ_CHAR_DIGIT,0,0,LJ_CHAR_GRAPH,0,0,0,0,
  LJ_CHAR_LOWER,0,0,0,LJ_CHAR_PUNCT,0,0,LJ_CHAR_SPACE,0,
  LJ_CHAR_UPPER,0,LJ_CHAR_ALNUM,LJ_CHAR_XDIGIT,0,0,0,0,0,0,0
];

fn match_class(c: u8, cl: u8) -> bool {
    if (cl & 0xc0) == 0x40 {
        let t = MATCH_CLASS_MAP[(cl & 0x1f) as usize];
        if t != 0 {
            let r = lj_char_isa(c, t);
            return if cl & 0x20 != 0 { r } else { !r };
        }
        if cl == b'z' {
            return c == 0;
        }
        if cl == b'Z' {
            return c != 0;
        }
    }
    cl == c
}

fn matchbracketclass(ms: &MatchState<'_, '_>, c: u8, mut p: usize, ec: usize) -> bool {
    let mut sig = true;
    if ms.p(p + 1) == b'^' {
        sig = false;
        p += 1; // skip the `^'
    }
    loop {
        p += 1;
        if p >= ec {
            break;
        }
        if ms.p(p) == L_ESC {
            p += 1;
            if match_class(c, ms.p(p)) {
                return sig;
            }
        } else if ms.p(p + 1) == b'-' && p + 2 < ec {
            p += 2;
            if ms.p(p - 2) <= c && c <= ms.p(p) {
                return sig;
            }
        } else if ms.p(p) == c {
            return sig;
        }
    }
    !sig
}

fn singlematch(ms: &MatchState<'_, '_>, c: u8, p: usize, ep: usize) -> bool {
    match ms.p(p) {
        b'.' => true, // matches any char
        L_ESC => match_class(c, ms.p(p + 1)),
        b'[' => matchbracketclass(ms, c, p, ep - 1),
        pc => pc == c,
    }
}

fn matchbalance(ms: &MatchState<'_, '_>, mut s: usize, p: usize) -> Option<usize> {
    if ms.p(p) == 0 || ms.p(p + 1) == 0 {
        crate::lua_error!("unbalanced pattern");
    }
    if ms.s(s) != ms.p(p) {
        return None;
    }
    let b = ms.p(p);
    let e = ms.p(p + 1);
    let mut cont = 1;
    loop {
        s += 1;
        if s >= ms.src_end() {
            break;
        }
        let c = ms.s(s);
        if c == e {
            cont -= 1;
            if cont == 0 {
                return Some(s + 1);
            }
        } else if c == b {
            cont += 1;
        }
    }
    None // string ends out of balance
}

fn max_expand(ms: &mut MatchState<'_, '_>, s: usize, p: usize, ep: usize) -> Option<usize> {
    let mut i: isize = 0; // counts maximum expand for item
    while s + (i as usize) < ms.src_end() && singlematch(ms, ms.s(s + i as usize), p, ep) {
        i += 1;
    }
    // keeps trying to match with the maximum repetitions
    while i >= 0 {
        if let Some(res) = do_match(ms, s + i as usize, ep + 1) {
            return Some(res);
        }
        i -= 1; // else didn't match; reduce 1 repetition to try again
    }
    None
}

fn min_expand(ms: &mut MatchState<'_, '_>, mut s: usize, p: usize, ep: usize) -> Option<usize> {
    loop {
        if let Some(res) = do_match(ms, s, ep + 1) {
            return Some(res);
        } else if s < ms.src_end() && singlematch(ms, ms.s(s), p, ep) {
            s += 1; // try with one more repetition
        } else {
            return None;
        }
    }
}

fn start_capture(ms: &mut MatchState<'_, '_>, s: usize, p: usize, what: isize) -> Option<usize> {
    let level = ms.level;
    if level >= LUA_MAXCAPTURES {
        crate::lua_error!("too many captures");
    }
    ms.capture[level] = (s, what);
    ms.level = level + 1;
    let res = do_match(ms, s, p);
    if res.is_none() {
        ms.level -= 1; // undo capture
    }
    res
}

fn end_capture(ms: &mut MatchState<'_, '_>, s: usize, p: usize) -> Option<usize> {
    let l = capture_to_close(ms);
    ms.capture[l].1 = (s - ms.capture[l].0) as isize; // close capture
    let res = do_match(ms, s, p);
    if res.is_none() {
        ms.capture[l].1 = CAP_UNFINISHED; // undo capture
    }
    res
}

fn match_capture(ms: &MatchState<'_, '_>, s: usize, l: u8) -> Option<usize> {
    let l = check_capture(ms, l);
    let (init, len) = ms.capture[l];
    let len = len as usize;
    if ms.src_end() - s >= len && ms.src[init..init + len] == ms.src[s..s + len] {
        Some(s + len)
    } else {
        None
    }
}

/// `match()`: `goto init` is the `continue` of the loop; recursion depth is
/// counted per call as in C.
fn do_match(ms: &mut MatchState<'_, '_>, mut s: usize, mut p: usize) -> Option<usize> {
    ms.depth += 1;
    if ms.depth > LJ_MAX_XLEVEL {
        crate::lua_error!("pattern too complex");
    }
    let res: Option<usize> = 'init: loop {
        match ms.p(p) {
            b'(' => {
                // start capture
                break if ms.p(p + 1) == b')' {
                    // position capture?
                    start_capture(ms, s, p + 2, CAP_POSITION)
                } else {
                    start_capture(ms, s, p + 1, CAP_UNFINISHED)
                };
            }
            b')' => break end_capture(ms, s, p + 1), // end capture
            0 => break Some(s),                      // end of pattern: match succeeded
            b'$' if ms.p(p + 1) == 0 => {
                // is the `$' the last char in pattern?
                break if s != ms.src_end() { None } else { Some(s) }; // check end of string
            }
            L_ESC if ms.p(p + 1) == b'b' => {
                // balanced string?
                match matchbalance(ms, s, p + 2) {
                    None => break None,
                    Some(ns) => {
                        s = ns;
                        p += 4;
                        continue 'init; // else s = match(ms, s, p+4);
                    }
                }
            }
            L_ESC if ms.p(p + 1) == b'f' => {
                // frontier?
                p += 2;
                if ms.p(p) != b'[' {
                    crate::lua_error!("missing '[' after '%f' in pattern");
                }
                let ep = classend(ms, p); // points to what is next
                let previous = if s == 0 { 0 } else { ms.s(s - 1) };
                if matchbracketclass(ms, previous, p, ep - 1)
                    || !matchbracketclass(ms, ms.s(s), p, ep - 1)
                {
                    break None;
                }
                p = ep;
                continue 'init; // else s = match(ms, s, ep);
            }
            L_ESC if lj_char_isdigit(ms.p(p + 1)) => {
                // capture results (%0-%9)?
                match match_capture(ms, s, ms.p(p + 1)) {
                    None => break None,
                    Some(ns) => {
                        s = ns;
                        p += 2;
                        continue 'init; // else s = match(ms, s, p+2)
                    }
                }
            }
            _ => {
                // default: it is a pattern item
                let ep = classend(ms, p); // points to what is next
                let m = s < ms.src_end() && singlematch(ms, ms.s(s), p, ep);
                match ms.p(ep) {
                    b'?' => {
                        // optional
                        if m {
                            if let Some(res) = do_match(ms, s + 1, ep + 1) {
                                break Some(res);
                            }
                        }
                        p = ep + 1;
                        continue 'init; // else s = match(ms, s, ep+1);
                    }
                    b'*' => break max_expand(ms, s, p, ep), // 0 or more repetitions
                    b'+' => {
                        break if m {
                            max_expand(ms, s + 1, p, ep)
                        } else {
                            None
                        }
                    } // 1 or more repetitions
                    b'-' => break min_expand(ms, s, p, ep), // 0 or more repetitions (minimum)
                    _ => {
                        if m {
                            s += 1;
                            p = ep;
                            continue 'init; // else s = match(ms, s+1, ep);
                        }
                        break None;
                    }
                }
            }
        }
    };
    ms.depth -= 1;
    res
}

fn push_onecapture<'s>(ms: &MatchState<'s, '_>, i: usize, s: usize, e: usize) -> Capture<'s> {
    if i >= ms.level {
        if i == 0 {
            // ms->level == 0, too: add whole match
            Capture::Str(&ms.src[s..e])
        } else {
            crate::lua_error!("invalid capture index");
        }
    } else {
        let (init, l) = ms.capture[i];
        if l == CAP_UNFINISHED {
            crate::lua_error!("unfinished capture");
        }
        if l == CAP_POSITION {
            Capture::Pos(init + 1)
        } else {
            Capture::Str(&ms.src[init..init + l as usize])
        }
    }
}

/// `push_captures(ms, s, e)`; `whole` = `s != NULL` in C.
fn push_captures<'s>(ms: &MatchState<'s, '_>, se: Option<(usize, usize)>) -> Captures<'s> {
    let nlevels = if ms.level == 0 && se.is_some() {
        1
    } else {
        ms.level
    };
    let (s, e) = se.unwrap_or((0, 0));
    (0..nlevels).map(|i| push_onecapture(ms, i, s, e)).collect()
}

/// `lj_str_haspattern`.
fn haspattern(p: &[u8]) -> bool {
    p.iter()
        .any(|&c| lj_char_isa(c, LJ_CHAR_PUNCT) && b"^$*+?.([%-".contains(&c))
}

/// `lj_str_find`: plain substring search.
fn str_find_plain(s: &[u8], p: &[u8]) -> Option<usize> {
    if p.is_empty() {
        return Some(0);
    }
    s.windows(p.len()).position(|w| w == p)
}

/// Start index (0-based) from Lua's 1-based, possibly negative `init`.
fn find_start(slen: usize, init: Option<i64>) -> usize {
    let mut start = init.unwrap_or(1);
    if start < 0 {
        start += slen as i64;
    } else {
        start -= 1;
    }
    if start < 0 {
        start = 0;
    }
    let st = start as usize;
    if st > slen {
        slen // !LJ_52: clamp
    } else {
        st
    }
}

/// `string.find(s, p, init, plain)` on bytes: `(start, end, captures)` with
/// 1-based inclusive positions.
pub fn find_bytes<'s>(
    s: &'s [u8],
    p: &[u8],
    init: Option<i64>,
    plain: bool,
) -> Option<(usize, usize, Captures<'s>)> {
    let st = find_start(s.len(), init);
    if plain || !haspattern(p) {
        // Search for fixed string.
        let q = str_find_plain(&s[st..], p)? + st;
        return Some((q + 1, q + p.len(), Captures::new()));
    }
    // Search for pattern.
    let (pstr, anchor) = if p.first() == Some(&b'^') {
        (&p[1..], true)
    } else {
        (p, false)
    };
    let mut sstr = st;
    loop {
        let mut ms = MatchState::new(s, pstr);
        if let Some(q) = do_match(&mut ms, sstr, 0) {
            return Some((sstr + 1, q, push_captures(&ms, None)));
        }
        let cont = sstr < s.len() && !anchor;
        sstr += 1;
        if !cont {
            return None;
        }
    }
}

/// `string.match(s, p, init)` on bytes.
pub fn match_bytes<'s>(s: &'s [u8], p: &[u8], init: Option<i64>) -> Option<Captures<'s>> {
    let st = find_start(s.len(), init);
    let (pstr, anchor) = if p.first() == Some(&b'^') {
        (&p[1..], true)
    } else {
        (p, false)
    };
    let mut sstr = st;
    loop {
        let mut ms = MatchState::new(s, pstr);
        if let Some(q) = do_match(&mut ms, sstr, 0) {
            return Some(push_captures(&ms, Some((sstr, q))));
        }
        let cont = sstr < s.len() && !anchor;
        sstr += 1;
        if !cont {
            return None;
        }
    }
}

/// `s:find(p, init, plain)`: `(start, end, captures)`, 1-based inclusive.
pub fn find<'s>(
    s: &'s str,
    p: &str,
    init: Option<i64>,
    plain: bool,
) -> Option<(usize, usize, Captures<'s>)> {
    find_bytes(s.as_bytes(), p.as_bytes(), init, plain)
}

/// `s:match(p, init)`.
pub fn r#match<'s>(s: &'s str, p: &str, init: Option<i64>) -> Option<Captures<'s>> {
    match_bytes(s.as_bytes(), p.as_bytes(), init)
}

/// `string.gmatch(s, p)`: a lazy iterator (`string_gmatch_aux`). `^` is not
/// an anchor in gmatch (it matches a literal `^`).
pub struct GMatch<'s, 'p> {
    src: &'s [u8],
    pat: &'p [u8],
    pos: usize,
}

impl<'s> Iterator for GMatch<'s, '_> {
    type Item = Captures<'s>;
    fn next(&mut self) -> Option<Captures<'s>> {
        let mut src = self.pos;
        while src <= self.src.len() {
            let mut ms = MatchState::new(self.src, self.pat);
            if let Some(e) = do_match(&mut ms, src, 0) {
                let mut pos = e;
                if e == src {
                    pos += 1; // Ensure progress for empty match.
                }
                self.pos = pos;
                return Some(push_captures(&ms, Some((src, e))));
            }
            src += 1;
        }
        self.pos = self.src.len() + 1;
        None // not found
    }
}

/// `s:gmatch(p)` on bytes.
pub fn gmatch_bytes<'s, 'p>(s: &'s [u8], p: &'p [u8]) -> GMatch<'s, 'p> {
    GMatch {
        src: s,
        pat: p,
        pos: 0,
    }
}

/// `s:gmatch(p)`.
pub fn gmatch<'s, 'p>(s: &'s str, p: &'p str) -> GMatch<'s, 'p> {
    gmatch_bytes(s.as_bytes(), p.as_bytes())
}

/// Replacement value for [`gsub_bytes_with`].
pub enum Repl<'r> {
    /// A string (or number already converted with `tostring`): `%0`-`%9`
    /// expand, `%x` → `x`.
    Str(&'r [u8]),
    /// A function of the captures; `None` keeps the original match (Lua
    /// `nil`/`false`).
    Func(&'r mut dyn FnMut(&Captures<'_>) -> Option<Vec<u8>>),
    /// A table indexed by the first capture (or the whole match); `None` keeps
    /// the original match.
    Table(&'r mut dyn FnMut(&Capture<'_>) -> Option<Vec<u8>>),
}

/// `add_s`: expand a replacement string.
fn add_s(ms: &MatchState<'_, '_>, b: &mut Vec<u8>, s: usize, e: usize, news: &[u8]) {
    let l = news.len();
    let mut i = 0usize;
    while i < l {
        if news[i] != L_ESC {
            b.push(news[i]);
        } else {
            i += 1; // skip ESC
            let c = at(news, i);
            if !lj_char_isdigit(c) {
                b.push(c);
            } else if c == b'0' {
                b.extend_from_slice(&ms.src[s..e]);
            } else {
                push_onecapture(ms, (c - b'1') as usize, s, e).push_to(b); // add capture to accumulated result
            }
        }
        i += 1;
    }
}

/// `string.gsub(s, p, repl, n)` on bytes; returns `(result, count)`.
pub fn gsub_bytes_with(
    src: &[u8],
    p: &[u8],
    mut repl: Repl<'_>,
    max_n: Option<i64>,
) -> (Vec<u8>, usize) {
    let srcl = src.len();
    let max_s = max_n.unwrap_or(srcl as i64 + 1);
    let (p, anchor) = if p.first() == Some(&b'^') {
        (&p[1..], true)
    } else {
        (p, false)
    };
    let mut n: i64 = 0;
    let mut b: Vec<u8> = Vec::with_capacity(srcl);
    let mut s = 0usize;
    while n < max_s {
        let mut ms = MatchState::new(src, p);
        let e = do_match(&mut ms, s, 0);
        if let Some(e) = e {
            n += 1;
            // add_value
            let val: Option<Vec<u8>> = match &mut repl {
                Repl::Str(news) => {
                    add_s(&ms, &mut b, s, e, news);
                    None
                }
                Repl::Func(f) => {
                    let caps = push_captures(&ms, Some((s, e)));
                    Some(f(&caps).unwrap_or_else(|| src[s..e].to_vec())) // nil or false? keep original text
                }
                Repl::Table(t) => {
                    let cap = push_onecapture(&ms, 0, s, e);
                    Some(t(&cap).unwrap_or_else(|| src[s..e].to_vec()))
                }
            };
            if let Some(v) = val {
                b.extend_from_slice(&v); // add result to accumulator
            }
        }
        match e {
            Some(e) if e > s => s = e, // non empty match? skip it
            _ if s < srcl => {
                b.push(src[s]);
                s += 1;
            }
            _ => break,
        }
        if anchor {
            break;
        }
    }
    b.extend_from_slice(&src[s..]);
    (b, n as usize)
}

fn into_string(b: Vec<u8>) -> String {
    match String::from_utf8(b) {
        Ok(s) => s,
        Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
    }
}

/// `s:gsub(p, repl, n)` with a string replacement.
pub fn gsub(s: &str, p: &str, repl: &str, n: Option<i64>) -> (String, usize) {
    let (b, c) = gsub_bytes_with(s.as_bytes(), p.as_bytes(), Repl::Str(repl.as_bytes()), n);
    (into_string(b), c)
}

/// `s:gsub(p, function(...) ... end, n)`; `None` keeps the match (nil/false).
pub fn gsub_fn(
    s: &str,
    p: &str,
    mut f: impl FnMut(&Captures<'_>) -> Option<String>,
    n: Option<i64>,
) -> (String, usize) {
    let mut g = |c: &Captures<'_>| f(c).map(String::into_bytes);
    let (b, c) = gsub_bytes_with(s.as_bytes(), p.as_bytes(), Repl::Func(&mut g), n);
    (into_string(b), c)
}

/// `s:gsub(p, tbl, n)`: `lookup` receives the first capture (or whole match).
pub fn gsub_table(
    s: &str,
    p: &str,
    mut lookup: impl FnMut(&Capture<'_>) -> Option<String>,
    n: Option<i64>,
) -> (String, usize) {
    let mut g = |c: &Capture<'_>| lookup(c).map(String::into_bytes);
    let (b, c) = gsub_bytes_with(s.as_bytes(), p.as_bytes(), Repl::Table(&mut g), n);
    (into_string(b), c)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps(s: &str, p: &str) -> Option<Vec<String>> {
        r#match(s, p, None).map(|v| {
            v.iter()
                .map(|c| match c {
                    Capture::Str(s) => String::from_utf8_lossy(s).into_owned(),
                    Capture::Pos(p) => format!("@{p}"),
                })
                .collect()
        })
    }

    #[test]
    fn basic_classes_and_quantifiers() {
        assert_eq!(
            caps("+43 to strength", "^([%+%-]?%d+) to (%a+)"),
            Some(vec!["+43".into(), "strength".into()])
        );
        assert_eq!(caps("abc", "b*"), Some(vec![String::new()]));
        assert_eq!(
            caps("hello world", "(%w+) (%w+)"),
            Some(vec!["hello".into(), "world".into()])
        );
        assert_eq!(caps("aaa", "a-b"), None);
        assert_eq!(caps("aaab", "a-b"), Some(vec!["aaab".into()]));
        assert_eq!(
            caps("x = 10", "()=()"),
            Some(vec!["@3".into(), "@4".into()])
        );
        assert_eq!(caps("f(a(b)c)d", "%b()"), Some(vec!["(a(b)c)".into()]));
        assert_eq!(
            caps("THE (quick) fox", "%f[%a]%a+"),
            Some(vec!["THE".into()])
        );
        assert_eq!(caps("[x]", "[%]]"), Some(vec!["]".to_owned()]));
        assert_eq!(caps("a-b", "[a%-]+"), Some(vec!["a-".into()]));
        assert_eq!(caps("abcabc", "(abc)%1"), Some(vec!["abc".into()]));
        assert_eq!(caps("a!b", "%g+"), Some(vec!["a!b".into()]));
    }

    #[test]
    fn find_and_gsub() {
        let (a, b, c) = find("10% increased damage", "(%d+)%% increased", None, false).unwrap();
        assert_eq!((a, b), (1, 13));
        assert_eq!(c.as_slice(), &[Capture::Str(b"10")]);
        assert_eq!(find("a.b", ".", None, true).unwrap().0, 2);
        assert_eq!(
            find("abc", "", Some(10), false).map(|f| (f.0, f.1)),
            Some((4, 3))
        );
        assert_eq!(gsub("hello world", "o", "0", None).0, "hell0 w0rld");
        assert_eq!(gsub("abc", "%w", "%0%0", None).0, "aabbcc");
        assert_eq!(gsub("  trim  ", "^%s*(.-)%s*$", "%1", None).0, "trim");
        assert_eq!(
            gsub_fn("a b", "%a", |c| Some(c[0].as_str().to_uppercase()), None).0,
            "A B"
        );
        assert_eq!(gsub("abc", "", "-", None), ("-a-b-c-".into(), 4));
        assert_eq!(
            gsub_table(
                "a b",
                "%a",
                |c| (c.as_str() == "a").then(|| "X".into()),
                None
            )
            .0,
            "X b"
        );
    }

    #[test]
    fn gmatch_all() {
        let got: Vec<String> = gmatch("a,b,,c", "([^,]*)")
            .map(|c| c[0].as_str().to_owned())
            .collect();
        assert_eq!(got, vec!["a", "", "b", "", "", "c", ""]);
    }
}
