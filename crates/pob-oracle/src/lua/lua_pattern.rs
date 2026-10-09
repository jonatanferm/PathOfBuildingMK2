//! Lua 5.1 string patterns (port of `lstrlib.c`'s matcher).
//!
//! PoB's parsers are tables of Lua patterns; matching them with the exact
//! Lua semantics (character classes, `-` lazy repetition, `%b`, `%f`, position
//! captures, back-references) lets those tables be carried over verbatim.
//! Operates on bytes, like Lua; `%a` etc. follow the C locale.

const MAXCAPTURES: usize = 32;
const L_ESC: u8 = b'%';
const CAP_UNFINISHED: isize = -1;
const CAP_POSITION: isize = -2;

/// A capture result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Capture<'s> {
    Str(&'s [u8]),
    /// `()` position capture: 1-based byte index.
    Pos(usize),
}

impl<'s> Capture<'s> {
    #[must_use]
    /// The capture as UTF-8 text (`""` for position captures or non-UTF-8).
    pub fn as_str(&self) -> &'s str {
        match self {
            Capture::Str(s) => std::str::from_utf8(s).unwrap_or(""),
            Capture::Pos(_) => "",
        }
    }

    #[must_use]
    pub fn as_bytes(&self) -> &'s [u8] {
        match self {
            Capture::Str(s) => s,
            Capture::Pos(_) => b"",
        }
    }

    /// Lua `tonumber(cap)`.
    #[must_use]
    pub fn num(&self) -> Option<f64> {
        match self {
            Capture::Str(s) => std::str::from_utf8(s).ok().and_then(to_number),
            Capture::Pos(p) => Some(*p as f64),
        }
    }
}

/// Lua `tonumber(s)` for decimal / hex strings (leading/trailing space allowed).
#[must_use]
pub fn to_number(s: &str) -> Option<f64> {
    let t = s.trim_matches(|c: char| c.is_ascii_whitespace());
    if t.is_empty() {
        return None;
    }
    let (neg, body) = match t.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    if let Some(hex) = body.strip_prefix("0x").or_else(|| body.strip_prefix("0X")) {
        let v = u64::from_str_radix(hex, 16).ok()? as f64;
        return Some(if neg { -v } else { v });
    }
    // Rust accepts "inf"/"nan"; Lua 5.1's strtod-based tonumber does too, but
    // PoB never relies on it — reject to stay conservative.
    if body
        .chars()
        .any(|c| c.is_ascii_alphabetic() && c != 'e' && c != 'E')
    {
        return None;
    }
    let v: f64 = body.parse().ok()?;
    Some(if neg { -v } else { v })
}

struct MatchState<'a> {
    src: &'a [u8],
    pat: &'a [u8],
    level: usize,
    capture: [(usize, isize); MAXCAPTURES],
    depth: u32,
}

#[derive(Debug)]
pub struct PatternError(pub &'static str);

type R<T> = Result<T, PatternError>;

fn class_end(ms: &MatchState<'_>, mut p: usize) -> R<usize> {
    let pat = ms.pat;
    let c = pat[p];
    p += 1;
    if c == L_ESC {
        if p >= pat.len() {
            return Err(PatternError("malformed pattern (ends with '%')"));
        }
        return Ok(p + 1);
    }
    if c == b'[' {
        if p < pat.len() && pat[p] == b'^' {
            p += 1;
        }
        loop {
            // look for a ']'
            if p >= pat.len() {
                return Err(PatternError("malformed pattern (missing ']')"));
            }
            let cc = pat[p];
            p += 1;
            if cc == L_ESC && p < pat.len() {
                p += 1; // skip escapes (e.g. '%]')
            }
            if p < pat.len() && pat[p] == b']' {
                return Ok(p + 1);
            }
            if p >= pat.len() {
                return Err(PatternError("malformed pattern (missing ']')"));
            }
        }
    }
    Ok(p)
}

fn single_class(c: u8, cl: u8) -> bool {
    let res = match cl.to_ascii_lowercase() {
        b'a' => c.is_ascii_alphabetic(),
        b'c' => c.is_ascii_control(),
        b'd' => c.is_ascii_digit(),
        b'l' => c.is_ascii_lowercase(),
        b'p' => c.is_ascii_punctuation(),
        b's' => matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c),
        b'u' => c.is_ascii_uppercase(),
        b'w' => c.is_ascii_alphanumeric(),
        b'x' => c.is_ascii_hexdigit(),
        b'z' => c == 0,
        _ => return cl == c,
    };
    if cl.is_ascii_uppercase() {
        !res
    } else {
        res
    }
}

fn match_bracket_class(ms: &MatchState<'_>, c: u8, mut p: usize, ec: usize) -> bool {
    let pat = ms.pat;
    let mut sig = true;
    if pat[p + 1] == b'^' {
        sig = false;
        p += 1; // skip the '^'
    }
    p += 1;
    while p < ec {
        if pat[p] == L_ESC {
            p += 1;
            if single_class(c, pat[p]) {
                return sig;
            }
            p += 1;
        } else if p + 2 < ec && pat[p + 1] == b'-' {
            if pat[p] <= c && c <= pat[p + 2] {
                return sig;
            }
            p += 3;
        } else {
            if pat[p] == c {
                return sig;
            }
            p += 1;
        }
    }
    !sig
}

fn single_match(ms: &MatchState<'_>, s: usize, p: usize, ep: usize) -> bool {
    if s >= ms.src.len() {
        return false;
    }
    let c = ms.src[s];
    match ms.pat[p] {
        b'.' => true,
        L_ESC => single_class(c, ms.pat[p + 1]),
        b'[' => match_bracket_class(ms, c, p, ep - 1),
        pc => pc == c,
    }
}

fn match_balance(ms: &MatchState<'_>, s: usize, p: usize) -> R<Option<usize>> {
    if p + 1 >= ms.pat.len() {
        return Err(PatternError("unbalanced pattern"));
    }
    if s >= ms.src.len() || ms.src[s] != ms.pat[p] {
        return Ok(None);
    }
    let (b, e) = (ms.pat[p], ms.pat[p + 1]);
    let mut cont = 1;
    let mut i = s + 1;
    while i < ms.src.len() {
        let c = ms.src[i];
        if c == e {
            cont -= 1;
            if cont == 0 {
                return Ok(Some(i + 1));
            }
        } else if c == b {
            cont += 1;
        }
        i += 1;
    }
    Ok(None)
}

fn max_expand(ms: &mut MatchState<'_>, s: usize, p: usize, ep: usize) -> R<Option<usize>> {
    let mut i = 0usize;
    while single_match(ms, s + i, p, ep) {
        i += 1;
    }
    // keeps trying to match with the maximum repetitions
    loop {
        if let Some(r) = do_match(ms, s + i, ep + 1)? {
            return Ok(Some(r));
        }
        if i == 0 {
            return Ok(None);
        }
        i -= 1;
    }
}

fn min_expand(ms: &mut MatchState<'_>, mut s: usize, p: usize, ep: usize) -> R<Option<usize>> {
    loop {
        if let Some(r) = do_match(ms, s, ep + 1)? {
            return Ok(Some(r));
        } else if single_match(ms, s, p, ep) {
            s += 1;
        } else {
            return Ok(None);
        }
    }
}

fn start_capture(ms: &mut MatchState<'_>, s: usize, p: usize, what: isize) -> R<Option<usize>> {
    if ms.level >= MAXCAPTURES {
        return Err(PatternError("too many captures"));
    }
    ms.capture[ms.level] = (s, what);
    ms.level += 1;
    let res = do_match(ms, s, p)?;
    if res.is_none() {
        ms.level -= 1; // undo capture
    }
    Ok(res)
}

fn capture_to_close(ms: &MatchState<'_>) -> R<usize> {
    let mut level = ms.level as isize - 1;
    while level >= 0 {
        if ms.capture[level as usize].1 == CAP_UNFINISHED {
            return Ok(level as usize);
        }
        level -= 1;
    }
    Err(PatternError("invalid pattern capture"))
}

fn end_capture(ms: &mut MatchState<'_>, s: usize, p: usize) -> R<Option<usize>> {
    let l = capture_to_close(ms)?;
    ms.capture[l].1 = (s - ms.capture[l].0) as isize; // close capture
    let res = do_match(ms, s, p)?;
    if res.is_none() {
        ms.capture[l].1 = CAP_UNFINISHED; // undo capture
    }
    Ok(res)
}

fn check_capture(ms: &MatchState<'_>, l: u8) -> R<usize> {
    let l = l.wrapping_sub(b'1') as usize;
    if l >= ms.level || ms.capture[l].1 == CAP_UNFINISHED {
        return Err(PatternError("invalid capture index"));
    }
    Ok(l)
}

fn match_capture(ms: &MatchState<'_>, s: usize, l: u8) -> R<Option<usize>> {
    let l = check_capture(ms, l)?;
    let (start, len) = ms.capture[l];
    let len = len as usize;
    if ms.src.len() - s >= len && ms.src[start..start + len] == ms.src[s..s + len] {
        Ok(Some(s + len))
    } else {
        Ok(None)
    }
}

fn do_match(ms: &mut MatchState<'_>, mut s: usize, mut p: usize) -> R<Option<usize>> {
    ms.depth += 1;
    if ms.depth > 200_000 {
        return Err(PatternError("pattern too complex"));
    }
    let res = loop {
        if p >= ms.pat.len() {
            break Ok(Some(s)); // end of pattern
        }
        match ms.pat[p] {
            b'(' => {
                break if p + 1 < ms.pat.len() && ms.pat[p + 1] == b')' {
                    start_capture(ms, s, p + 2, CAP_POSITION)
                } else {
                    start_capture(ms, s, p + 1, CAP_UNFINISHED)
                };
            }
            b')' => break end_capture(ms, s, p + 1),
            L_ESC if p + 1 < ms.pat.len() => match ms.pat[p + 1] {
                b'b' => match match_balance(ms, s, p + 2)? {
                    Some(ns) => {
                        s = ns;
                        p += 4;
                        continue;
                    }
                    None => break Ok(None),
                },
                b'f' => {
                    p += 2;
                    if p >= ms.pat.len() || ms.pat[p] != b'[' {
                        break Err(PatternError("missing '[' after '%f' in pattern"));
                    }
                    let ep = class_end(ms, p)?;
                    let prev = if s == 0 { 0 } else { ms.src[s - 1] };
                    let cur = if s < ms.src.len() { ms.src[s] } else { 0 };
                    if match_bracket_class(ms, prev, p, ep - 1)
                        || !match_bracket_class(ms, cur, p, ep - 1)
                    {
                        break Ok(None);
                    }
                    p = ep;
                    continue;
                }
                d if d.is_ascii_digit() => match match_capture(ms, s, d)? {
                    Some(ns) => {
                        s = ns;
                        p += 2;
                        continue;
                    }
                    None => break Ok(None),
                },
                _ => {
                    // default
                    match default_step(ms, &mut s, &mut p)? {
                        Step::Continue => continue,
                        Step::Done(r) => break Ok(r),
                    }
                }
            },
            b'$' if p + 1 == ms.pat.len() => {
                break Ok(if s == ms.src.len() { Some(s) } else { None });
            }
            _ => match default_step(ms, &mut s, &mut p)? {
                Step::Continue => continue,
                Step::Done(r) => break Ok(r),
            },
        }
    };
    ms.depth -= 1;
    res
}

enum Step {
    Continue,
    Done(Option<usize>),
}

fn default_step(ms: &mut MatchState<'_>, s: &mut usize, p: &mut usize) -> R<Step> {
    let ep = class_end(ms, *p)?;
    let m = single_match(ms, *s, *p, ep);
    let next = if ep < ms.pat.len() { ms.pat[ep] } else { 0 };
    match next {
        b'?' => {
            if m {
                if let Some(r) = do_match(ms, *s + 1, ep + 1)? {
                    return Ok(Step::Done(Some(r)));
                }
            }
            *p = ep + 1;
            Ok(Step::Continue)
        }
        b'*' => Ok(Step::Done(max_expand(ms, *s, *p, ep)?)),
        b'+' => Ok(Step::Done(if m {
            max_expand(ms, *s + 1, *p, ep)?
        } else {
            None
        })),
        b'-' => Ok(Step::Done(min_expand(ms, *s, *p, ep)?)),
        _ => {
            if !m {
                return Ok(Step::Done(None));
            }
            *s += 1;
            *p = ep;
            Ok(Step::Continue)
        }
    }
}

fn get_capture<'s>(
    ms: &MatchState<'_>,
    src: &'s [u8],
    i: usize,
    s: usize,
    e: usize,
) -> R<Capture<'s>> {
    if i >= ms.level {
        if i == 0 {
            // ms->level == 0, too: whole match
            return Ok(Capture::Str(&src[s..e]));
        }
        return Err(PatternError("invalid capture index"));
    }
    let (start, len) = ms.capture[i];
    if len == CAP_UNFINISHED {
        return Err(PatternError("unfinished capture"));
    }
    if len == CAP_POSITION {
        return Ok(Capture::Pos(start + 1));
    }
    Ok(Capture::Str(&src[start..start + len as usize]))
}

fn push_captures<'s>(
    ms: &MatchState<'_>,
    src: &'s [u8],
    s: usize,
    e: usize,
    whole_if_none: bool,
) -> R<Vec<Capture<'s>>> {
    let n = if ms.level == 0 && whole_if_none {
        1
    } else {
        ms.level
    };
    (0..n).map(|i| get_capture(ms, src, i, s, e)).collect()
}

/// Result of [`find`]: 1-based inclusive `start..=end` plus captures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found<'s> {
    pub start: usize,
    pub end: usize,
    pub captures: Vec<Capture<'s>>,
}

fn new_state<'a>(src: &'a [u8], pat: &'a [u8]) -> MatchState<'a> {
    MatchState {
        src,
        pat,
        level: 0,
        capture: [(0, 0); MAXCAPTURES],
        depth: 0,
    }
}

/// `string.find(s, pattern, init, plain)`; `init` is 1-based (negative from end).
pub fn find<'s>(s: &'s [u8], pattern: &[u8], init: i64, plain: bool) -> R<Option<Found<'s>>> {
    let src = s;
    let len = src.len() as i64;
    let mut init = if init < 0 { len + init + 1 } else { init };
    if init < 1 {
        init = 1;
    }
    if init > len + 1 {
        return Ok(None);
    }
    let init = (init - 1) as usize;
    let special = pattern.iter().any(|b| b"^$*+?.([%-".contains(b));
    if plain || !special {
        let hay = &src[init..];
        let needle = pattern;
        let pos = if needle.is_empty() {
            Some(0)
        } else {
            hay.windows(needle.len()).position(|w| w == needle)
        };
        return Ok(pos.map(|p| Found {
            start: init + p + 1,
            end: init + p + needle.len(),
            captures: Vec::new(),
        }));
    }
    let pat = pattern;
    let (anchor, pstart) = if pat.first() == Some(&b'^') {
        (true, 1)
    } else {
        (false, 0)
    };
    let mut s1 = init;
    loop {
        let mut ms = new_state(src, pat);
        if let Some(e) = do_match(&mut ms, s1, pstart)? {
            return Ok(Some(Found {
                start: s1 + 1,
                end: e,
                captures: push_captures(&ms, s, s1, e, false)?,
            }));
        }
        s1 += 1;
        if anchor || s1 > src.len() {
            return Ok(None);
        }
    }
}

/// `string.match(s, pattern, init)`: captures (or the whole match).
pub fn matches<'s>(s: &'s [u8], pattern: &[u8], init: i64) -> R<Option<Vec<Capture<'s>>>> {
    let src = s;
    let len = src.len() as i64;
    let mut init = if init < 0 { len + init + 1 } else { init };
    if init < 1 {
        init = 1;
    }
    if init > len + 1 {
        return Ok(None);
    }
    let pat = pattern;
    let (anchor, pstart) = if pat.first() == Some(&b'^') {
        (true, 1)
    } else {
        (false, 0)
    };
    let mut s1 = (init - 1) as usize;
    loop {
        let mut ms = new_state(src, pat);
        if let Some(e) = do_match(&mut ms, s1, pstart)? {
            return Ok(Some(push_captures(&ms, s, s1, e, true)?));
        }
        s1 += 1;
        if anchor || s1 > src.len() {
            return Ok(None);
        }
    }
}

/// `string.gmatch(s, pattern)` collected eagerly.
pub fn gmatch<'s>(s: &'s [u8], pattern: &[u8]) -> R<Vec<Vec<Capture<'s>>>> {
    let src = s;
    let pat = pattern;
    let mut out = Vec::new();
    let mut src_pos = 0usize;
    while src_pos <= src.len() {
        let mut ms = new_state(src, pat);
        match do_match(&mut ms, src_pos, 0)? {
            Some(e) => {
                let caps = push_captures(&ms, s, src_pos, e, true)?;
                out.push(caps);
                src_pos = if e == src_pos { e + 1 } else { e };
            }
            None => src_pos += 1,
        }
    }
    Ok(out)
}

/// Replacement for [`gsub`].
pub enum Repl<'r> {
    /// `%0`–`%9` expanded, `%%` → `%`.
    Str(&'r [u8]),
    /// Called with the captures; `None` keeps the original match (Lua `nil`/`false`).
    Func(&'r mut dyn FnMut(&[Capture<'_>]) -> Option<Vec<u8>>),
}

/// `string.gsub(s, pattern, repl, n)`; returns `(result, count)`.
pub fn gsub(
    s: &[u8],
    pattern: &[u8],
    mut repl: Repl<'_>,
    max_n: Option<usize>,
) -> R<(Vec<u8>, usize)> {
    let src = s;
    let pat = pattern;
    let (anchor, pstart) = if pat.first() == Some(&b'^') {
        (true, 1)
    } else {
        (false, 0)
    };
    let max_n = max_n.unwrap_or(src.len() + 1);
    let mut out: Vec<u8> = Vec::with_capacity(src.len());
    let mut src_pos = 0usize;
    let mut n = 0usize;
    while n < max_n {
        let mut ms = new_state(src, pat);
        let e = do_match(&mut ms, src_pos, pstart)?;
        if let Some(e) = e {
            n += 1;
            let caps = push_captures(&ms, s, src_pos, e, true)?;
            let whole = &src[src_pos..e];
            match &mut repl {
                Repl::Str(r) => {
                    let rb: &[u8] = r;
                    let mut i = 0;
                    while i < rb.len() {
                        if rb[i] == L_ESC && i + 1 < rb.len() {
                            i += 1;
                            let d = rb[i];
                            if d.is_ascii_digit() {
                                if d == b'0' {
                                    out.extend_from_slice(whole);
                                } else {
                                    let idx = (d - b'1') as usize;
                                    let cap = caps
                                        .get(idx)
                                        .ok_or(PatternError("invalid capture index"))?;
                                    match cap {
                                        Capture::Str(c) => out.extend_from_slice(c),
                                        Capture::Pos(p) => {
                                            out.extend_from_slice(p.to_string().as_bytes())
                                        }
                                    }
                                }
                            } else {
                                out.push(d);
                            }
                        } else {
                            out.push(rb[i]);
                        }
                        i += 1;
                    }
                }
                Repl::Func(f) => match f(&caps) {
                    Some(r) => out.extend_from_slice(&r),
                    None => out.extend_from_slice(whole),
                },
            }
            if e > src_pos {
                src_pos = e;
            } else {
                if src_pos < src.len() {
                    out.push(src[src_pos]);
                }
                src_pos += 1;
            }
        } else if src_pos < src.len() {
            out.push(src[src_pos]);
            src_pos += 1;
        } else {
            break;
        }
        if src_pos > src.len() || anchor {
            break;
        }
    }
    if src_pos < src.len() {
        out.extend_from_slice(&src[src_pos..]);
    }
    Ok((out, n))
}

/// Convenience: `s:gsub(pattern, repl)` on text.
#[must_use]
pub fn gsub_str(s: &str, pattern: &str, repl: &str) -> String {
    gsub(
        s.as_bytes(),
        pattern.as_bytes(),
        Repl::Str(repl.as_bytes()),
        None,
    )
    .map_or_else(
        |_| s.to_owned(),
        |(r, _)| String::from_utf8_lossy(&r).into_owned(),
    )
}

/// Convenience: first capture of `s:match(pattern)` as `&str`.
#[must_use]
pub fn match1<'s>(s: &'s str, pattern: &str) -> Option<&'s str> {
    matches(s.as_bytes(), pattern.as_bytes(), 1)
        .ok()
        .flatten()
        .and_then(|c| c.into_iter().next())
        .map(|c| c.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps(s: &str, p: &str) -> Option<Vec<String>> {
        matches(s.as_bytes(), p.as_bytes(), 1).unwrap().map(|v| {
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
        assert_eq!(caps("abc", "b*"), Some(vec!["".into()]));
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
    }

    #[test]
    fn find_and_gsub() {
        let f = find(b"10% increased damage", b"(%d+)%% increased", 1, false)
            .unwrap()
            .unwrap();
        assert_eq!((f.start, f.end), (1, 13));
        assert_eq!(f.captures, vec![Capture::Str(b"10")]);
        assert_eq!(find(b"a.b", b".", 1, true).unwrap().unwrap().start, 2);
        assert_eq!(gsub_str("hello world", "o", "0"), "hell0 w0rld");
        assert_eq!(gsub_str("abc", "%w", "%0%0"), "aabbcc");
        assert_eq!(gsub_str("  trim  ", "^%s*(.-)%s*$", "%1"), "trim");
        let mut up = |c: &[Capture<'_>]| Some(c[0].as_str().to_uppercase().into_bytes());
        assert_eq!(
            gsub(b"a b", b"%a", Repl::Func(&mut up), None).unwrap().0,
            b"A B"
        );
        assert_eq!(
            gsub(b"abc", b"", Repl::Str(b"-"), None).unwrap().0,
            b"-a-b-c-"
        );
    }

    #[test]
    fn gmatch_all() {
        let v = gmatch(b"a,b,,c", b"([^,]*)").unwrap();
        let got: Vec<&str> = v.iter().map(|c| c[0].as_str()).collect();
        assert_eq!(got, vec!["a", "", "b", "", "", "c", ""]);
    }

    #[test]
    fn tonumber() {
        assert_eq!(to_number(" 12 "), Some(12.0));
        assert_eq!(to_number("-1.5"), Some(-1.5));
        assert_eq!(to_number("0x10"), Some(16.0));
        assert_eq!(to_number("abc"), None);
        assert_eq!(to_number(""), None);
    }
}
