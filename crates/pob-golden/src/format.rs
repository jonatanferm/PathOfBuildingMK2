//! The golden file grammar `golden/1` (GOLDEN-PLAN §3.2).
//!
//! ```text
//! {"golden":1,"set":"e2e.ca","key":"all57/allflame_exp_000","input":{…},"origin":{…},
//!  "recorder":"rust-port@<sha>","tolerance":"num_eq","lines":1927}        <- line 1: the header
//! ["main.Life",4312]                                                     <- an E2E line
//! ["Adds 1 to 2 Fire Damage",null,{"mods":[…],"extra":null}]             <- a query line
//! ```
//!
//! * Line 1 is the [`Header`]. Its `lines` is the number of lines that follow.
//! * An E2E line is `[key, value]`, a query line `[key, input, value]`; one file holds one kind.
//! * Lines are sorted by key, bytewise, and keys are unique.
//! * Every line is compact JSON with sorted object keys and canonical numbers
//!   ([`crate::encode::canonical`]), so equal content is equal bytes.
//! * A path ending in `.zst` is a zstd stream of the same text (a shard, §2.2).

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value as J;

use crate::encode::canonical;
use crate::{bail, Error, Result};

/// The grammar version this crate reads and writes.
pub const GOLDEN_VERSION: u64 = 1;

/// The tolerance name of §3.5 (`num_eq`), the only one there is.
pub const TOLERANCE_NUM_EQ: &str = "num_eq";

/// zstd level of a shard: 19 (the plan's measurements use it).
pub const ZSTD_LEVEL: i32 = 19;

/// Line 1 of a golden file. Field order is the serialisation order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Header {
    /// [`GOLDEN_VERSION`].
    pub golden: u64,
    /// The set (`e2e.ca`, `fn.parser.lines`, `fuzz.defence`, …).
    pub set: String,
    /// The file's key in its set: a build key (`all57/allflame_exp_000`) or a shard name.
    pub key: String,
    /// What the file was recorded from: `{"xml": "builds/…", "blake3": "…", "rounds": 2}` for a
    /// build, a digest of generated inputs for a fuzz shard, …
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<J>,
    /// `{"pob": "v2.67.2@16de4b82d", "luajit": {"arch", "numberMode", "jit"}, "runs": K}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<J>,
    /// `rust-port@<sha>`: the recorder's commit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recorder: Option<String>,
    /// [`TOLERANCE_NUM_EQ`].
    pub tolerance: String,
    /// The number of lines after the header.
    pub lines: usize,
    /// Set-specific header fields (a support list, an order table, …).
    #[serde(flatten)]
    pub extra: BTreeMap<String, J>,
}

impl Header {
    /// A header for `set` / `key` with nothing else filled in.
    #[must_use]
    pub fn new(set: &str, key: &str) -> Self {
        Self {
            golden: GOLDEN_VERSION,
            set: set.to_owned(),
            key: key.to_owned(),
            input: None,
            origin: None,
            recorder: None,
            tolerance: TOLERANCE_NUM_EQ.to_owned(),
            lines: 0,
            extra: BTreeMap::new(),
        }
    }

    /// The build XML this file was recorded from and its BLAKE3 (`input.xml`, `input.blake3`).
    #[must_use]
    pub fn input_xml(&self) -> Option<(&str, Option<&str>)> {
        let i = self.input.as_ref()?;
        let xml = i.get("xml")?.as_str()?;
        Some((xml, i.get("blake3").and_then(J::as_str)))
    }

    /// `input.rounds`: the number of BuildOutput calls PoB's load made (§3.4).
    #[must_use]
    pub fn rounds(&self) -> Option<u64> {
        self.input.as_ref()?.get("rounds")?.as_u64()
    }
}

/// One line after the header.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub key: String,
    /// `None`: an E2E line `[key, value]`. `Some`: a query line `[key, input, value]` (the input
    /// may be `null`, when the key is the input).
    pub input: Option<J>,
    pub value: J,
}

impl Line {
    #[must_use]
    pub fn e2e(key: impl Into<String>, value: J) -> Self {
        Self {
            key: key.into(),
            input: None,
            value,
        }
    }

    #[must_use]
    pub fn query(key: impl Into<String>, input: J, value: J) -> Self {
        Self {
            key: key.into(),
            input: Some(input),
            value,
        }
    }

    /// The canonical text of the line (no newline).
    #[must_use]
    pub fn text(&self) -> String {
        let key = J::String(self.key.clone());
        let arr = match &self.input {
            None => vec![key, self.value.clone()],
            Some(i) => vec![key, i.clone(), self.value.clone()],
        };
        canonical(&J::Array(arr))
    }

    fn parse(text: &str, n: usize) -> Result<Self> {
        let v: J = serde_json::from_str(text).map_err(|e| Error(format!("line {n}: {e}")))?;
        let J::Array(mut a) = v else {
            bail!("line {n}: not an array");
        };
        let key = match a.first() {
            Some(J::String(s)) => s.clone(),
            _ => bail!("line {n}: the key is not a string"),
        };
        match a.len() {
            2 => Ok(Self::e2e(key, a.pop().unwrap_or(J::Null))),
            3 => {
                let value = a.pop().unwrap_or(J::Null);
                let input = a.pop().unwrap_or(J::Null);
                Ok(Self::query(key, input, value))
            }
            k => bail!("line {n}: {k} elements (want 2 or 3)"),
        }
    }
}

/// A golden file: the header and its lines, sorted by key.
#[derive(Debug, Clone, PartialEq)]
pub struct GoldenFile {
    pub header: Header,
    pub lines: Vec<Line>,
}

impl GoldenFile {
    /// A file of `header`'s set and key; [`Self::to_text`] sorts the lines and sets the count.
    #[must_use]
    pub fn new(header: Header, lines: Vec<Line>) -> Self {
        Self { header, lines }
    }

    /// Parse and validate a file's text: the header, the line count, one line kind, the key
    /// order (strictly increasing, bytewise).
    pub fn parse(text: &str) -> Result<Self> {
        let mut it = text.split('\n');
        let first = it.next().unwrap_or_default();
        let header: Header =
            serde_json::from_str(first).map_err(|e| Error(format!("header: {e}")))?;
        if header.golden != GOLDEN_VERSION {
            bail!(
                "header: golden/{} (this reader is golden/{GOLDEN_VERSION})",
                header.golden
            );
        }
        if header.tolerance != TOLERANCE_NUM_EQ {
            bail!("header: unknown tolerance {:?}", header.tolerance);
        }
        let mut lines = Vec::with_capacity(header.lines);
        for (i, l) in it.enumerate() {
            if l.is_empty() {
                continue;
            }
            let line = Line::parse(l, i + 2)?;
            if let Some(prev) = lines.last() {
                let prev: &Line = prev;
                if prev.key >= line.key {
                    bail!(
                        "line {}: key {:?} is not after {:?} (sorted, unique keys)",
                        i + 2,
                        line.key,
                        prev.key
                    );
                }
                if prev.input.is_some() != line.input.is_some() {
                    bail!("line {}: E2E and query lines mixed", i + 2);
                }
            }
            lines.push(line);
        }
        if lines.len() != header.lines {
            bail!(
                "header says {} lines, the file has {}",
                header.lines,
                lines.len()
            );
        }
        Ok(Self { header, lines })
    }

    /// Read a plain or `.zst` file.
    pub fn read(path: &Path) -> Result<Self> {
        let text = read_text(path)?;
        Self::parse(&text).map_err(|e| Error(format!("{}: {e}", path.display())))
    }

    /// The canonical text: the header (with `lines` set), then the lines sorted by key, each
    /// followed by a newline. Fails on a duplicate key.
    pub fn to_text(&self) -> Result<String> {
        let mut lines: Vec<&Line> = self.lines.iter().collect();
        lines.sort_by(|a, b| a.key.cmp(&b.key));
        if let Some(w) = lines.windows(2).find(|w| w[0].key == w[1].key) {
            bail!(
                "{} {}: duplicate key {:?}",
                self.header.set,
                self.header.key,
                w[0].key
            );
        }
        if lines
            .windows(2)
            .any(|w| w[0].input.is_some() != w[1].input.is_some())
        {
            bail!(
                "{} {}: E2E and query lines mixed",
                self.header.set,
                self.header.key
            );
        }
        let mut header = self.header.clone();
        header.lines = lines.len();
        let mut out = serde_json::to_string(&header).map_err(|e| Error(e.to_string()))?;
        out.push('\n');
        for l in lines {
            out.push_str(&l.text());
            out.push('\n');
        }
        Ok(out)
    }

    /// The bytes of the file at `path`: the text, zstd-compressed for a `.zst` path.
    pub fn to_bytes(&self, path: &Path) -> Result<Vec<u8>> {
        let text = self.to_text()?;
        if is_zst(path) {
            zstd::encode_all(text.as_bytes(), ZSTD_LEVEL).map_err(|e| Error(e.to_string()))
        } else {
            Ok(text.into_bytes())
        }
    }

    /// Write the file atomically; returns its bytes.
    pub fn write(&self, path: &Path) -> Result<Vec<u8>> {
        let bytes = self.to_bytes(path)?;
        crate::write_atomic(path, &bytes)?;
        Ok(bytes)
    }

    /// The line of `key`.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Line> {
        self.lines
            .binary_search_by(|l| l.key.as_str().cmp(key))
            .ok()
            .map(|i| &self.lines[i])
    }
}

/// Whether `path` names a zstd shard.
#[must_use]
pub fn is_zst(path: &Path) -> bool {
    path.extension().is_some_and(|e| e == "zst")
}

/// The text of a golden file's bytes (decompressed for a `.zst` path).
pub fn text_of(path: &Path, bytes: &[u8]) -> Result<String> {
    let raw = if is_zst(path) {
        zstd::decode_all(bytes).map_err(|e| Error(format!("{}: zstd: {e}", path.display())))?
    } else {
        bytes.to_vec()
    };
    String::from_utf8(raw).map_err(|e| Error(format!("{}: {e}", path.display())))
}

/// The text of a plain or `.zst` file.
pub fn read_text(path: &Path) -> Result<String> {
    text_of(path, &crate::read_bytes(path)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample() -> GoldenFile {
        let mut h = Header::new("e2e.ca", "all57/x");
        h.input = Some(json!({"xml": "builds/all57/x.xml", "blake3": "ab", "rounds": 2}));
        h.origin = Some(json!({"pob": "v2.67.2@16de4b82d", "runs": 3}));
        GoldenFile::new(
            h,
            vec![
                Line::e2e("main.Life", json!(4312.0)),
                Line::e2e("main.Armour", json!({"$num": "inf"})),
                Line::e2e("calcs.BuffList", json!("Onslaught")),
            ],
        )
    }

    #[test]
    fn round_trip_plain_and_zst() {
        let f = sample();
        let text = f.to_text().unwrap();
        let mut lines = text.lines();
        assert_eq!(
            lines.next().unwrap(),
            r#"{"golden":1,"set":"e2e.ca","key":"all57/x","input":{"blake3":"ab","rounds":2,"xml":"builds/all57/x.xml"},"origin":{"pob":"v2.67.2@16de4b82d","runs":3},"tolerance":"num_eq","lines":3}"#
        );
        assert_eq!(lines.next().unwrap(), r#"["calcs.BuffList","Onslaught"]"#);
        assert_eq!(lines.next().unwrap(), r#"["main.Armour",{"$num":"inf"}]"#);
        assert_eq!(lines.next().unwrap(), r#"["main.Life",4312]"#);
        let back = GoldenFile::parse(&text).unwrap();
        assert_eq!(back.header.lines, 3);
        assert_eq!(back.header.rounds(), Some(2));
        assert_eq!(
            back.header.input_xml(),
            Some(("builds/all57/x.xml", Some("ab")))
        );
        assert_eq!(back.get("main.Life").unwrap().value, json!(4312));
        assert_eq!(back.to_text().unwrap(), text);

        let dir = std::env::temp_dir().join(format!("pob-golden-fmt-{}", std::process::id()));
        let p = dir.join("a.jsonl.zst");
        let bytes = f.write(&p).unwrap();
        assert_ne!(bytes, text.as_bytes());
        assert_eq!(GoldenFile::read(&p).unwrap().to_text().unwrap(), text);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn query_lines_and_errors() {
        let f = GoldenFile::new(
            Header::new("fn.parser.lines", "all"),
            vec![
                Line::query("b", J::Null, json!({"mods": []})),
                Line::query("a", json!({"x": 1}), json!(true)),
            ],
        );
        let text = f.to_text().unwrap();
        assert!(text.contains("\n[\"a\",{\"x\":1},true]\n[\"b\",null,{\"mods\":[]}]\n"));
        let back = GoldenFile::parse(&text).unwrap();
        assert_eq!(back.lines[1].input, Some(J::Null));

        let dup = GoldenFile::new(
            Header::new("s", "k"),
            vec![Line::e2e("a", json!(1)), Line::e2e("a", json!(2))],
        );
        assert!(dup.to_text().unwrap_err().0.contains("duplicate"));
        let h = r#"{"golden":1,"set":"s","key":"k","tolerance":"num_eq","lines":2}"#;
        let unsorted = format!("{h}\n[\"b\",1]\n[\"a\",1]\n");
        assert!(GoldenFile::parse(&unsorted)
            .unwrap_err()
            .0
            .contains("not after"));
        let short = format!("{h}\n[\"a\",1]\n");
        assert!(GoldenFile::parse(&short).unwrap_err().0.contains("says 2"));
        let mixed = format!("{h}\n[\"a\",1]\n[\"b\",null,1]\n");
        assert!(GoldenFile::parse(&mixed).unwrap_err().0.contains("mixed"));
        let v2 = r#"{"golden":2,"set":"s","key":"k","tolerance":"num_eq","lines":0}"#;
        assert!(GoldenFile::parse(v2).unwrap_err().0.contains("golden/2"));
        // set-specific header fields survive
        let x =
            r#"{"golden":1,"set":"s","key":"k","tolerance":"num_eq","lines":0,"supports":["a"]}"#;
        let f = GoldenFile::parse(x).unwrap();
        assert_eq!(f.header.extra["supports"], json!(["a"]));
        assert_eq!(f.to_text().unwrap().trim_end(), x);
    }
}
