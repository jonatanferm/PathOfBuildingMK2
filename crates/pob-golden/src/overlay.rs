//! Bless overlays (GOLDEN-PLAN §2.4, §3.6).
//!
//! An intentional behaviour change never rewrites PoB's recording. It goes into an overlay file
//! beside it, `overlay/<the base path without .jsonl[.zst]>.bless.jsonl`, one line per changed
//! value, sorted by key:
//!
//! ```text
//! [key, pob_value, new_value, reason, commit]
//! ```
//!
//! `pob_value` is the base file's value (`{"$absent":true}` for a key the base file lacks: a key
//! Rust now adds on purpose); `new_value` is what Rust is expected to produce from now on
//! (`{"$absent":true}` for a key it no longer produces, `{"$error":…}` when it now fails). When
//! the runner applies an overlay, a line whose `pob_value` is not the base file's value is
//! stale and fails, so a re-recorded base cannot silently keep an old bless.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::Value as J;

use crate::encode::{absent, canonical};
use crate::format::{GoldenFile, Line};
use crate::{bail, Error, Result};

/// One overlay line.
#[derive(Debug, Clone, PartialEq)]
pub struct OverlayLine {
    pub key: String,
    pub pob: J,
    pub new: J,
    pub reason: String,
    pub commit: String,
}

impl OverlayLine {
    #[must_use]
    pub fn text(&self) -> String {
        canonical(&J::Array(vec![
            J::String(self.key.clone()),
            self.pob.clone(),
            self.new.clone(),
            J::String(self.reason.clone()),
            J::String(self.commit.clone()),
        ]))
    }
}

/// The overlay of one golden file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Overlay {
    pub lines: BTreeMap<String, OverlayLine>,
}

/// The overlay path of a base file (both relative to the goldens directory).
#[must_use]
pub fn overlay_path(base: &str) -> String {
    let stem = base
        .strip_suffix(".jsonl.zst")
        .or_else(|| base.strip_suffix(".jsonl"))
        .unwrap_or(base);
    format!("overlay/{stem}.bless.jsonl")
}

impl Overlay {
    pub fn parse(text: &str) -> Result<Self> {
        let mut lines = BTreeMap::new();
        for (i, l) in text.lines().enumerate() {
            if l.is_empty() {
                continue;
            }
            let v: J =
                serde_json::from_str(l).map_err(|e| Error(format!("line {}: {e}", i + 1)))?;
            let a = v.as_array().filter(|a| a.len() == 5);
            let Some(a) = a else {
                bail!(
                    "line {}: not [key, pob_value, new_value, reason, commit]",
                    i + 1
                );
            };
            let s = |j: usize| a[j].as_str().map(str::to_owned);
            let (Some(key), Some(reason), Some(commit)) = (s(0), s(3), s(4)) else {
                bail!("line {}: key, reason and commit must be strings", i + 1);
            };
            if reason.trim().is_empty() {
                bail!("line {}: empty reason", i + 1);
            }
            let line = OverlayLine {
                key: key.clone(),
                pob: a[1].clone(),
                new: a[2].clone(),
                reason,
                commit,
            };
            if lines.insert(key.clone(), line).is_some() {
                bail!("line {}: duplicate key {key:?}", i + 1);
            }
        }
        Ok(Self { lines })
    }

    /// Read an overlay; a missing file is an empty overlay.
    pub fn read(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(t) => Self::parse(&t).map_err(|e| Error(format!("{}: {e}", path.display()))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(Error(format!("{}: {e}", path.display()))),
        }
    }

    #[must_use]
    pub fn to_text(&self) -> String {
        let mut s = String::new();
        for l in self.lines.values() {
            s.push_str(&l.text());
            s.push('\n');
        }
        s
    }

    pub fn write(&self, path: &Path) -> Result<()> {
        crate::write_atomic(path, self.to_text().as_bytes())
    }

    /// The file the runner compares with: `base` with every overlay line applied. The second
    /// value lists the stale overlay keys (their `pob_value` is not the base file's).
    #[must_use]
    pub fn apply(&self, base: &GoldenFile) -> (GoldenFile, Vec<String>) {
        let mut out = base.clone();
        let mut stale = Vec::new();
        let query = base.lines.first().is_some_and(|l| l.input.is_some());
        let mut added = Vec::new();
        for ol in self.lines.values() {
            match out.lines.binary_search_by(|l| l.key.as_str().cmp(&ol.key)) {
                Ok(i) => {
                    if canonical(&out.lines[i].value) == canonical(&ol.pob) {
                        out.lines[i].value = ol.new.clone();
                    } else {
                        stale.push(ol.key.clone());
                    }
                }
                Err(_) => {
                    if ol.pob == absent() {
                        added.push(Line {
                            key: ol.key.clone(),
                            input: query.then_some(J::Null),
                            value: ol.new.clone(),
                        });
                    } else {
                        stale.push(ol.key.clone());
                    }
                }
            }
        }
        if !added.is_empty() {
            out.lines.extend(added);
            out.lines.sort_by(|a, b| a.key.cmp(&b.key));
        }
        out.header.lines = out.lines.len();
        (out, stale)
    }
}

/// `*` (any run) and `?` (any one character) glob over a whole string.
#[must_use]
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let (p, t): (Vec<char>, Vec<char>) = (pattern.chars().collect(), text.chars().collect());
    let (mut pi, mut ti) = (0, 0);
    let (mut star, mut mark) = (None, 0);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            pi += 1;
            mark = ti;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// `POB_GOLDEN_FILTER=<set glob>[:<key glob>]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filter {
    pub set: String,
    pub key: Option<String>,
}

impl Filter {
    #[must_use]
    pub fn parse(s: &str) -> Self {
        match s.split_once(':') {
            Some((a, b)) => Self {
                set: a.to_owned(),
                key: Some(b.to_owned()),
            },
            None => Self {
                set: s.to_owned(),
                key: None,
            },
        }
    }

    #[must_use]
    pub fn matches_set(&self, set: &str) -> bool {
        glob_match(&self.set, set)
    }

    #[must_use]
    pub fn matches(&self, set: &str, key: &str) -> bool {
        self.matches_set(set) && self.key.as_deref().map_or(true, |k| glob_match(k, key))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::Header;
    use serde_json::json;

    #[test]
    fn paths_and_globs() {
        assert_eq!(
            overlay_path("e2e/ca/all57/x.jsonl"),
            "overlay/e2e/ca/all57/x.bless.jsonl"
        );
        assert_eq!(
            overlay_path("fn/lut/lut.3.jsonl.zst"),
            "overlay/fn/lut/lut.3.bless.jsonl"
        );
        assert!(glob_match("main.*", "main.Life"));
        assert!(glob_match("*Life", "main.Life"));
        assert!(glob_match("m?in.*Li*", "main.MainHand.Life"));
        assert!(!glob_match("main.*", "calcs.Life"));
        assert!(glob_match("*", ""));
        assert!(!glob_match("a", ""));
        let f = Filter::parse("e2e.ca:main.*");
        assert!(f.matches("e2e.ca", "main.Life") && !f.matches("e2e.ca", "calcs.x"));
        assert!(!f.matches("e2e.int3", "main.Life"));
        assert!(Filter::parse("e2e.*").matches("e2e.bm", "anything"));
    }

    #[test]
    fn apply_and_stale() {
        let base = GoldenFile::new(
            Header::new("e2e.ca", "k"),
            vec![Line::e2e("a", json!(1)), Line::e2e("b", json!(2))],
        );
        let line = |key: &str, pob: J, new: J| OverlayLine {
            key: key.into(),
            pob,
            new,
            reason: "Life rounding follows PoB 2.68 (#123)".into(),
            commit: "abc".into(),
        };
        let mut o = Overlay::default();
        o.lines.insert("a".into(), line("a", json!(1), json!(1.5)));
        o.lines
            .insert("c".into(), line("c", absent(), json!("new")));
        let (eff, stale) = o.apply(&base);
        assert!(stale.is_empty());
        assert_eq!(eff.get("a").unwrap().value, json!(1.5));
        assert_eq!(eff.get("b").unwrap().value, json!(2));
        assert_eq!(eff.get("c").unwrap().value, json!("new"));
        assert_eq!(eff.header.lines, 3);
        // text round trip
        let back = Overlay::parse(&o.to_text()).unwrap();
        assert_eq!(back, o);
        // a re-recorded base makes the line stale
        let mut o2 = Overlay::default();
        o2.lines.insert("b".into(), line("b", json!(3), json!(4)));
        o2.lines.insert("z".into(), line("z", json!(1), json!(4)));
        let (_, stale) = o2.apply(&base);
        assert_eq!(stale, ["b", "z"]);
        assert!(Overlay::parse("[\"a\",1,2,\"\",\"c\"]\n")
            .unwrap_err()
            .0
            .contains("empty reason"));
        assert!(Overlay::parse("[\"a\",1,2]\n").is_err());
    }
}
