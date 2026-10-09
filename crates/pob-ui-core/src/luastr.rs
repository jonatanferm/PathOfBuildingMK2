//! UI-only byte-string helpers (ui doc §8 rules 4, 5; §12.3 U5). Owner: U5.
//!
//! `pob_lua` (the calc plan's crate) holds Lua's string semantics; this file adds what UI ports
//! need on top: expression-position forms (§8 rule 5: a call inside an expression keeps only its
//! first value) and, for U5, the caret-safe sub/insert EditControl needs and `tostring` glue.
//!
//! UI-U0: the two expression-position forms ui doc §9.2 (NotesTab) uses (CCR-0013: §9.2 imports
//! them from `pob_lua`, whose `gsub` returns `(String, count)` and whose `match` returns captures).

/// `s:gsub(p, repl)` in expression position: the string only.
pub fn gsub(s: &str, p: &str, repl: &str) -> String {
    pob_lua::gsub(s, p, repl, None).0
}

/// `s:match(p)` in expression position: the first capture (or the whole match) as a string; nil
/// becomes None. A position capture `()` shows as its number (`Capture::to_str`); callers that
/// need the number use `pob_lua::lmatch` directly.
pub fn lmatch(s: &str, p: &str) -> Option<String> {
    let caps = pob_lua::lmatch(s, p, None)?;
    caps.first().map(|c| c.to_str().into_owned())
}
