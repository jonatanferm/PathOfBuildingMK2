//! @pob-file Modules/Common.lua @16de4b82d
//! LuaJIT `string.*` byte semantics (calc doc §4.1 `string.rs`) and PoB's
//! string helpers from `Modules/Common.lua` (`sanitiseText` 306,
//! `string:split` 1001, `string:matchOrPattern` 1038), `ModParser.lua` (`firstToUpper` 13) and
//! `_SimpleGraphic.def.lua` (`StripEscapes` 295).
//!
//! Byte-level functions take and return `&[u8]`/`Vec<u8>` and are exact. The
//! `&str` functions are wrappers; where a byte operation could split a UTF-8
//! character the result is re-validated lossily (ui doc §8 rule 4) — PoB's
//! inputs are ASCII after `sanitiseText`.

use std::borrow::Cow;

pub use crate::pattern::{find, gmatch, gsub, gsub_fn, gsub_table, r#match, Capture, Captures};

/// `string.lower` (`lj_buf_putstr_lower`: ASCII only).
pub fn lower_bytes(s: &[u8]) -> Vec<u8> {
    s.iter()
        .map(|&c| if c.is_ascii_uppercase() { c + 0x20 } else { c })
        .collect()
}

/// `string.upper` (`lj_buf_putstr_upper`: ASCII only).
pub fn upper_bytes(s: &[u8]) -> Vec<u8> {
    s.iter()
        .map(|&c| if c.is_ascii_lowercase() { c - 0x20 } else { c })
        .collect()
}

/// `s:lower()`.
pub fn lower(s: &str) -> String {
    s.to_ascii_lowercase()
}

/// `s:upper()`.
pub fn upper(s: &str) -> String {
    s.to_ascii_uppercase()
}

/// `string.sub` index arithmetic (`vm_x64.dasc` `string_sub`): the 0-based
/// half-open byte range, or `None` for the empty string.
pub fn sub_range(len: usize, i: i64, j: Option<i64>) -> Option<(usize, usize)> {
    let len = len as i64;
    let mut start = i;
    let mut end = j.unwrap_or(-1);
    if end < 0 {
        end += len + 1; // end = end+(len+1)
    } else if end > len {
        end = len; // end = len
    }
    if start <= 0 {
        if start != 0 {
            start += len + 1; // start = start+(len+1)
        }
        if start <= 0 {
            start = 1;
        }
    }
    if start > end {
        return None;
    }
    Some(((start - 1) as usize, end as usize))
}

/// `string.sub(s, i, j)` on bytes (1-based, negative from the end).
pub fn sub_bytes(s: &[u8], i: i64, j: Option<i64>) -> &[u8] {
    match sub_range(s.len(), i, j) {
        Some((a, b)) => &s[a..b],
        None => b"",
    }
}

/// `s:sub(i, j)`. Borrowed when the byte range falls on UTF-8 character
/// boundaries (always the case for ASCII, i.e. PoB text after
/// `sanitiseText`); when it splits a multi-byte character (possible on raw
/// item text, unique names, ...) the cut bytes are re-validated lossily
/// (ui doc §8 rule 4) instead of aborting the calc. Use [`sub_bytes`] when
/// the exact bytes matter.
pub fn sub(s: &str, i: i64, j: Option<i64>) -> Cow<'_, str> {
    match sub_range(s.len(), i, j) {
        Some((a, b)) => match s.get(a..b) {
            Some(cut) => Cow::Borrowed(cut),
            None => String::from_utf8_lossy(&s.as_bytes()[a..b]),
        },
        None => Cow::Borrowed(""),
    }
}

/// `s:sub(i, j)` re-validated lossily (same as [`sub`], kept for callers
/// that want to name the lossy behaviour explicitly).
pub fn sub_lossy(s: &str, i: i64, j: Option<i64>) -> Cow<'_, str> {
    sub(s, i, j)
}

/// `string.rep(s, n)` (`n <= 0` gives "").
pub fn rep(s: &str, n: usize) -> String {
    s.repeat(n)
}

/// `string.rep(s, n, sep)`.
pub fn rep_sep(s: &str, n: i64, sep: &str) -> String {
    if n <= 0 {
        return String::new();
    }
    let mut out = String::with_capacity((s.len() + sep.len()) * n as usize);
    for k in 0..n {
        if k > 0 {
            out.push_str(sep);
        }
        out.push_str(s);
    }
    out
}

/// `string.byte(s, i)`: the first returned value (`None` = no values).
pub fn byte(s: &str, i: i64) -> Option<u8> {
    byte_bytes(s.as_bytes(), i)
}

/// `string.byte(s, i)` on bytes.
pub fn byte_bytes(s: &[u8], i: i64) -> Option<u8> {
    let len = s.len() as i64;
    let mut start = i;
    let mut stop = i;
    if stop < 0 {
        stop += len + 1;
    }
    if start < 0 {
        start += len + 1;
    }
    if start <= 0 {
        start = 1;
    }
    if stop > len {
        stop = len;
    }
    if start > stop {
        return None; // Empty interval: return no results.
    }
    Some(s[(start - 1) as usize])
}

/// `string.char(...)`.
pub fn char_bytes(cs: &[u8]) -> Vec<u8> {
    cs.to_vec()
}

/// `string.reverse(s)` on bytes.
pub fn reverse_bytes(s: &[u8]) -> Vec<u8> {
    s.iter().rev().copied().collect()
}

/// `#s`.
pub fn len(s: &str) -> usize {
    s.len()
}

// Class function to split a string on a single character (??) separator.
// returns a list of fields, not including the separator.
// Will return the first field as blank if the first character of the string is the separator
// Separator defaults to colon
/// @pob Modules/Common.lua:997-1011 h=be11302e
pub fn split(s: &str, sep: &str) -> Vec<String> {
    // Initially from http://lua-users.org/wiki/SplitJoin
    // function will ignore duplicate separators
    // L1004 (`sep or ":"`: pass ":" for nil)
    let mut fields: Vec<String> = Vec::new();
    // L1005
    let pattern = crate::lfmt!("([^%s]+)", sep);
    // inject a blank entry if self begins with a colon
    // L1007
    if sub_bytes(s.as_bytes(), 1, Some(1)) == sep.as_bytes() {
        fields.push(String::new());
    }
    // L1008
    gsub_fn(
        s,
        &pattern,
        |c| {
            fields.push(c[0].to_str().into_owned());
            None
        },
        None,
    );
    // L1009
    fields
}

/// lua: Common.lua:1038-1079 `string:matchOrPattern(pattern)`: `s` matches one of the
/// alternatives (`a|b`, with `(..)` groups expanded) of `pattern`. Raises (`lua_error!`) like
/// `string.match` on a malformed alternative; a caller that `PCall`s it wraps it in
/// [`crate::error::catch`]. The one port every caller shares (FUP7; CCR-0107 §3, CCR-0112 §2,
/// ui doc §13 item 15).
///
/// ORDER: `pairs(subGroup.patterns)` (L1055) and `pairs(orPatterns)` (L1073) walk string keys in
/// LuaJIT hash order; here they walk insertion order (each key once, as in the `= true` set). The
/// result is an any-match, so the order shows only when one alternative matches and another
/// raises.
/// @pob Modules/Common.lua:1038-1079 h=f7b8bae7
pub fn matchOrPattern(s: &str, pattern: &str) -> bool {
    struct SubGroup {
        open: usize,
        close: usize,
        patterns: Vec<String>,
    }
    // L1039
    fn generateOrPatterns(pattern: &str) -> Vec<String> {
        // L1040
        let mut subGroups: Vec<SubGroup> = Vec::new();
        // L1041
        let mut index = 1usize;
        // find and call generate patterns on all subGroups
        // L1043
        let groups: Vec<String> = gmatch(pattern, "%b()")
            .map(|c| c[0].to_str().into_owned())
            .collect();
        for subGroup in groups {
            // L1044 (`subGroups[index]` is always nil here: index is #subGroups + 1, so the
            // search starts at 1)
            let init = subGroups.get(index - 1).map_or(1, |g| g.close) as i64;
            let (open, close) = match find(pattern, &subGroup, Some(init), true) {
                Some((a, b, _)) => (a, b),
                None => {
                    crate::lua_error!("attempt to perform arithmetic on local 'open' (a nil value)")
                }
            };
            // L1045
            let inner = sub(&subGroup, 2, Some(-2)).into_owned();
            subGroups.push(SubGroup {
                open,
                close,
                patterns: generateOrPatterns(&inner),
            });
            // L1046
            index += 1;
        }

        // generate complete patterns from the subGroup patterns
        // L1050
        let first_open = subGroups.first().map_or(0, |g| g.open) as i64;
        let mut generatedPatterns: Vec<String> =
            vec![sub(pattern, 1, Some(first_open - 1)).into_owned()];
        // L1051
        for (i, subGroup) in subGroups.iter().enumerate() {
            // L1052
            let next_open = subGroups.get(i + 1).map_or(0, |g| g.open) as i64;
            let regularNextString =
                sub(pattern, subGroup.close as i64 + 1, Some(next_open - 1)).into_owned();
            // L1053
            let mut tempPatterns: Vec<String> = Vec::new();
            // L1054
            for subPattern in &generatedPatterns {
                // L1055 (ORDER: insertion order, see above)
                for subGroupPattern in &subGroup.patterns {
                    // L1056
                    tempPatterns.push(format!("{subPattern}{subGroupPattern}{regularNextString}"));
                }
            }
            // L1059
            generatedPatterns = tempPatterns;
        }

        // apply | operators
        // L1063
        let mut orPatterns: Vec<String> = Vec::new();
        // L1064
        for generatedPattern in &generatedPatterns {
            // L1065
            for c in gmatch(generatedPattern, "[^|]+") {
                // L1066 store string as key to avoid duplicates.
                let orPattern = c[0].to_str().into_owned();
                if !orPatterns.contains(&orPattern) {
                    orPatterns.push(orPattern);
                }
            }
        }
        // L1069
        orPatterns
    }

    // L1072
    let orPatterns = generateOrPatterns(pattern);
    // L1073 (ORDER: insertion order, see above)
    for orPattern in &orPatterns {
        // L1074
        if r#match(s, orPattern, None).is_some() {
            // L1075
            return true;
        }
    }
    // L1078
    false
}

/// Clean item text by removing or replacing unsupported or redundant characters or sequences
/// @pob Modules/Common.lua:303-329 h=e58537d0
pub fn sanitiseText_bytes(text: &[u8]) -> Vec<u8> {
    use crate::pattern::{find_bytes, gsub_bytes_with, Repl};
    let g = |t: Vec<u8>, p: &[u8], r: &[u8]| gsub_bytes_with(&t, p, Repl::Str(r), None).0;
    // Something something unicode support something grumble
    // Only do these replacements if a char from 128-255 or '<' is found first
    // L310
    if find_bytes(text, b"[\x80-\xff<]", None, false).is_none() {
        return text.to_vec();
    }
    let mut t = g(text.to_vec(), b"%b<>", b"");
    t = g(t, b"\xe2\x80\x90", b"-"); // U+2010 HYPHEN
    t = g(t, b"\xe2\x80\x91", b"-"); // U+2011 NON-BREAKING HYPHEN
    t = g(t, b"\xe2\x80\x92", b"-"); // U+2012 FIGURE DASH
    t = g(t, b"\xe2\x80\x93", b"-"); // U+2013 EN DASH
    t = g(t, b"\xe2\x80\x94", b"-"); // U+2014 EM DASH
    t = g(t, b"\xe2\x80\x95", b"-"); // U+2015 HORIZONTAL BAR
    t = g(t, b"\xe2\x88\x92", b"-"); // U+2212 MINUS SIGN
    t = g(t, b"\xc3\xa4", b"a"); // U+00E4 LATIN SMALL LETTER A WITH DIAERESIS
    t = g(t, b"\xc3\xb6", b"o"); // U+00F6 LATIN SMALL LETTER O WITH DIAERESIS
                                 // single-byte: Windows-1252 and similar
    t = g(t, b"\x96", b"-"); // U+2013 EN DASH
    t = g(t, b"\x97", b"-"); // U+2014 EM DASH
    t = g(t, b"\xe4", b"a"); // U+00E4 LATIN SMALL LETTER A WITH DIAERESIS
    t = g(t, b"\xf6", b"o"); // U+00F6 LATIN SMALL LETTER O WITH DIAERESIS
                             // unsupported
    g(t, b"[\x80-\xff]", b"?")
}

/// `sanitiseText(text)` (the result is always ASCII when a replacement ran):
/// the `&str` form of [`sanitiseText_bytes`] (Modules/Common.lua:303-329; one
/// provenance marker per Lua range, so the marker is on the byte function).
pub fn sanitiseText(text: &str) -> String {
    let b = sanitiseText_bytes(text.as_bytes());
    match String::from_utf8(b) {
        Ok(s) => s,
        Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
    }
}

/// `firstToUpper(str)`: `(str:gsub("^%l", string.upper))`.
/// @pob Modules/ModParser.lua:13-15 h=c6d98bbf
pub fn firstToUpper(s: &str) -> String {
    // L14
    gsub_fn(s, "^%l", |c| Some(upper(c[0].as_str())), None).0
}

/// `StripEscapes(text)`: remove `^N` and `^xRRGGBB` colour codes.
/// @pob _SimpleGraphic.def.lua:295-298 h=363d3688
pub fn StripEscapes(text: &str) -> String {
    // L296
    let (s, _) = gsub(text, "%^%d", "", None);
    gsub(&s, "%^x%x%x%x%x%x%x", "", None).0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basics() {
        assert_eq!(sub("hello", 2, Some(4)), "ell");
        assert_eq!(sub("hello", -3, None), "llo");
        assert_eq!(sub("hello", 0, Some(0)), "");
        assert_eq!(sub("hello", -100, Some(2)), "he");
        assert_eq!(sub("hello", 4, Some(100)), "lo");
        // A cut through a multi-byte character does not panic.
        assert!(matches!(sub("a\u{e9}b", 1, Some(2)), Cow::Owned(_)));
        assert_eq!(sub("a\u{e9}b", 1, Some(2)), "a\u{fffd}");
        assert_eq!(sub("a\u{e9}b", 2, Some(3)), "\u{e9}");
        assert!(matches!(sub("a\u{e9}b", 2, Some(3)), Cow::Borrowed(_)));
        assert_eq!(byte("abc", -1), Some(b'c'));
        assert_eq!(byte("abc", 0), None);
        assert_eq!(rep_sep("a", 3, ","), "a,a,a");
        assert_eq!(split("a:b::c", ":"), vec!["a", "b", "c"]);
        assert_eq!(split(":a", ":"), vec!["", "a"]);
        assert_eq!(sanitiseText("a\u{2013}b<x>c"), "a-bc");
        assert_eq!(sanitiseText("plain"), "plain");
        assert_eq!(sanitiseText("Mj\u{f6}lner\u{e9}"), "Mjolner??");
        assert_eq!(firstToUpper("fire"), "Fire");
        assert_eq!(StripEscapes("^1a^xFF00FFb"), "ab");
    }
}
