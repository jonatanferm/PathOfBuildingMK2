//! `parse_oracle.jsonl`: PoB's own `modLib.parseMod` over every mod line
//! PoB parses, with the winning pattern (and its `Modules/ModParser.lua`
//! line) of every `scan()` call.
//!
//! The work happens in `lua/parse_oracle.lua` (see its header for how
//! ModParser is instrumented in memory without changing any line number).
//! The record format is documented in `FORMAT.md`.

use std::path::Path;

use anyhow::{anyhow, Context, Result};
use mlua::{Function, Table, Value};
use serde_json::{Map, Value as Json};

use crate::{lerr, sha256_hex, Env, FileInfo};

const PARSE_ORACLE: &str = include_str!("lua/parse_oracle.lua");

/// Output file name inside the dump directory.
pub const FILE_NAME: &str = "parse_oracle.jsonl";

/// Corpus parts, in the order they run.
pub const PARTS: &[&str] = &[
    "modCache",
    "trees",
    "uniques",
    "catalysts",
    "rares",
    "affixes",
    "enchantments",
    "cluster",
    "bases",
    "legion",
    "foulborn",
    "pantheons",
];

#[derive(Debug, Clone)]
pub struct Options {
    /// Run only these corpus parts (all of [`PARTS`] when `None`).
    pub parts: Option<Vec<String>>,
    /// Parse only every n-th `Data/ModCache.lua` line.
    pub mod_cache_stride: Option<usize>,
    /// Skip patterns whose required literal text is absent from the line
    /// (never changes a result; `false` is for the equivalence test).
    pub prefilter: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            parts: None,
            mod_cache_stride: None,
            prefilter: true,
        }
    }
}

/// The oracle's output: one JSON object per line (sorted by mod line) and a
/// summary (`records`, `withMods`, `nilMods`, `withExtra`, `errors`,
/// `fullTies`, `jewelFuncTies`, `unresolvedSteps`, `unresolvedKeys`,
/// `counts` (new lines per source), `runErrors`, `listSizes`, ...).
pub struct ParseOracle {
    pub lines: Vec<String>,
    pub info: Json,
}

impl ParseOracle {
    /// The file contents: every record followed by `\n`.
    pub fn bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.lines.iter().map(|l| l.len() + 1).sum());
        for l in &self.lines {
            out.extend_from_slice(l.as_bytes());
            out.push(b'\n');
        }
        out
    }

    /// Write `parse_oracle.jsonl` into `dir`.
    pub fn write(&self, dir: &Path) -> Result<FileInfo> {
        std::fs::create_dir_all(dir)?;
        let data = self.bytes();
        let path = dir.join(FILE_NAME);
        std::fs::write(&path, &data).with_context(|| format!("write {}", path.display()))?;
        Ok(FileInfo {
            name: FILE_NAME.into(),
            roots: vec![],
            bytes: data.len() as u64,
            sha256: sha256_hex(&data),
        })
    }
}

/// Plain JSON image of a small Lua value (the oracle's summary table).
#[allow(clippy::many_single_char_names)]
fn to_json(v: &Value) -> Result<Json> {
    Ok(match v {
        Value::Nil => Json::Null,
        Value::Boolean(b) => Json::Bool(*b),
        Value::Integer(i) => Json::from(*i),
        Value::Number(n) => {
            if n.fract() == 0.0 && n.abs() < 9.0e15 {
                Json::from(*n as i64)
            } else {
                serde_json::Number::from_f64(*n).map_or(Json::Null, Json::Number)
            }
        }
        Value::String(s) => Json::from(s.to_string_lossy()),
        Value::Table(t) => {
            let n = t.raw_len();
            if n > 0 {
                let mut a = Vec::with_capacity(n);
                for v in t.clone().sequence_values::<Value>() {
                    a.push(to_json(&v.map_err(lerr)?)?);
                }
                Json::Array(a)
            } else {
                let mut m = Map::new();
                for pair in t.clone().pairs::<Value, Value>() {
                    let (k, v) = pair.map_err(lerr)?;
                    let k = match k {
                        Value::String(s) => s.to_string_lossy(),
                        other => format!("{other:?}"),
                    };
                    m.insert(k, to_json(&v)?);
                }
                Json::Object(m)
            }
        }
        other => Json::from(format!("<{}>", other.type_name())),
    })
}

/// Boot PoB, load `Modules/ModTools` (which loads the instrumented
/// `Modules/ModParser`) plus `Modules/ItemTools` and `Modules/CalcTools` as
/// `Modules/Main.lua` does, and run the corpus.
pub fn run(pob_src: &Path, opts: &Options) -> Result<ParseOracle> {
    let env = Env::new(pob_src)?;
    let po: Table = env
        .lua
        .load(PARSE_ORACLE)
        .set_name("=pob-dump/parse_oracle.lua")
        .eval()
        .map_err(lerr)?;
    po.get::<Function>("install")
        .map_err(lerr)?
        .call::<()>(())
        .map_err(lerr)?;
    env.boot_pob()?;
    env.lua
        .load(
            r#"
LoadModule("Modules/ModTools")
require("Modules.ItemTools")
require("Modules.CalcTools")
"#,
        )
        .set_name("=pob-dump/parse_oracle-boot")
        .exec()
        .map_err(|e| anyhow!("loading ModTools/ItemTools/CalcTools: {e}"))?;
    let w = env.walker()?;
    w.set("pretty", false).map_err(lerr)?;
    let lopts = env.lua.create_table().map_err(lerr)?;
    if let Some(parts) = &opts.parts {
        let set = env.lua.create_table().map_err(lerr)?;
        for p in parts {
            if !PARTS.contains(&p.as_str()) {
                return Err(anyhow!(
                    "unknown parse oracle part {p:?} (known: {PARTS:?})"
                ));
            }
            set.set(p.as_str(), true).map_err(lerr)?;
        }
        lopts.set("parts", set).map_err(lerr)?;
    }
    if let Some(n) = opts.mod_cache_stride {
        lopts.set("modCacheStride", n.max(1)).map_err(lerr)?;
    }
    lopts.set("prefilter", opts.prefilter).map_err(lerr)?;
    let (out, info): (Table, Value) = po
        .get::<Function>("run")
        .map_err(lerr)?
        .call((w, lopts))
        .map_err(|e| anyhow!("parse oracle: {e}"))?;
    let mut lines = Vec::with_capacity(out.raw_len());
    for s in out.sequence_values::<mlua::String>() {
        lines.push(s.map_err(lerr)?.to_str().map_err(lerr)?.to_owned());
    }
    let info = to_json(&info)?;
    Ok(ParseOracle { lines, info })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `requiredLiteral` (the scan prefilter) on hand-checked patterns.
    #[test]
    fn required_literal() {
        let env = Env::new(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
        let po: Table = env.lua.load(PARSE_ORACLE).eval().unwrap();
        let f: Function = po.get("requiredLiteral").unwrap();
        let req = |p: &str| -> Option<String> { f.call::<Option<String>>(p).unwrap() };
        assert_eq!(req("(%d+)%% increased").as_deref(), Some("% increased"));
        assert_eq!(
            req("^([%+%-]?%d+)%% chance to").as_deref(),
            Some("% chance to")
        );
        assert_eq!(
            req("^adds (%d+) to (%d+) (%a+) damage").as_deref(),
            Some(" damage")
        );
        assert_eq!(
            req("[%a%s]* modifier magnitudes are doubled").as_deref(),
            Some(" modifier magnitudes are doubled")
        );
        assert_eq!(
            req("^skills cost life instead of (%d+)%% of mana cost$").as_deref(),
            Some("skills cost life instead of ")
        );
        // quantified items end a run
        assert_eq!(
            req("fires? an additional arrows?").as_deref(),
            Some(" an additional arrow")
        );
        assert_eq!(req("ab+cd").as_deref(), Some("ab"));
        assert_eq!(req("x+y"), None);
        assert_eq!(req("ab.cd").as_deref(), Some("ab"));
        // %b, %f, sets, escapes; ^/$ are literal away from the ends
        assert_eq!(req("a%b()bc").as_deref(), Some("bc"));
        assert_eq!(req("%f[%w]cold").as_deref(), Some("cold"));
        assert_eq!(req("[^%]]xyz").as_deref(), Some("xyz"));
        assert_eq!(req("%.5 per"), Some(".5 per".into()));
        assert_eq!(req("a$b").as_deref(), Some("a$b"));
        assert_eq!(req("%d+"), None);
        // every literal it returns really is required: it occurs in a match
        for (p, s) in [
            ("(%d+)%% increased", "12% increased damage"),
            ("^([%+%-]?%d+)%% chance to", "-5% chance to x"),
            ("fires? an additional arrows?", "fire an additional arrow"),
            ("a%b()bc", "a(x)bc"),
        ] {
            let need = req(p).unwrap();
            assert!(s.contains(&need), "{p:?} -> {need:?} not in {s:?}");
        }
    }
}
