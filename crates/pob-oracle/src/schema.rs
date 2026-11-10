//! The probe dump formats (calc-architecture.md §9.2) and helpers to read
//! them from Rust.
//!
//! * `pob-state/1` ([`StateDump`]): one probe hit, written by
//!   `lua/state_dump.lua`. The encoding is documented there and in the
//!   crate README: crates/pob-dump/FORMAT.md values and table members
//!   (`$seq`, `$num`, `$keys`, `$meta`, `$float`, `$bytes`, `$$`-escaped
//!   keys) plus an identity graph (`"$id": n` on the first occurrence of a
//!   shared table, `{"$ref": n}` elsewhere) and markers (`$data`, `$node`,
//!   `$treeNode`, `$object`, `$class`, `$env`, `$foreign`, `$fn`), mod stores
//!   (`$store`) and items (`$item`).
//! * `pob-hits/1` ([`HitsIndex`]): per-probe hit counts and the list of
//!   dumped hits of one run.
//! * `pob-provenance/1` ([`Provenance`]): last writer `file:line` of every
//!   key of the main env's proxied output tables.
//!
//! [`resolve_ids`] turns the integer identity graph into path references
//! (`{"$ref": "<path of the first occurrence>"}`), which is what the diff
//! and nondeterminism comparison work on: it makes two dumps comparable even
//! when an earlier difference shifted the id numbering.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value as J};

use crate::oracle::{OracleError, OracleOutput, Result};

pub const STATE_FORMAT: &str = "pob-state/1";
pub const HITS_FORMAT: &str = "pob-hits/1";
pub const PROVENANCE_FORMAT: &str = "pob-provenance/1";

/// A JSON list that Lua may have written as `{}` when empty.
pub fn de_list<'de, D: Deserializer<'de>, T: serde::de::DeserializeOwned>(
    d: D,
) -> std::result::Result<Vec<T>, D::Error> {
    match J::deserialize(d)? {
        J::Array(a) => a
            .into_iter()
            .map(|x| serde_json::from_value(x).map_err(serde::de::Error::custom))
            .collect(),
        J::Object(m) if m.is_empty() => Ok(Vec::new()),
        J::Null => Ok(Vec::new()),
        other => Err(serde::de::Error::custom(format!(
            "expected a list, got {other}"
        ))),
    }
}

/// A JSON map that Lua may have written as `[]`… never, but `{}` or null.
pub fn de_map<'de, D: Deserializer<'de>, T: serde::de::DeserializeOwned>(
    d: D,
) -> std::result::Result<BTreeMap<String, T>, D::Error> {
    match J::deserialize(d)? {
        J::Object(m) => m
            .into_iter()
            .map(|(k, x)| {
                serde_json::from_value(x)
                    .map(|v| (k, v))
                    .map_err(serde::de::Error::custom)
            })
            .collect(),
        J::Null => Ok(BTreeMap::new()),
        J::Array(a) if a.is_empty() => Ok(BTreeMap::new()),
        other => Err(serde::de::Error::custom(format!(
            "expected a map, got {other}"
        ))),
    }
}

fn de_opt_string<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<Option<String>, D::Error> {
    Ok(match J::deserialize(d)? {
        J::String(s) => Some(s),
        _ => None,
    })
}

/// Read a file, transparently decompressing `*.zst`.
pub fn read_text(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path).map_err(|e| OracleError(format!("{}: {e}", path.display())))?;
    if path.extension().is_some_and(|e| e == "zst") {
        let raw = zstd::decode_all(&bytes[..])
            .map_err(|e| OracleError(format!("{}: {e}", path.display())))?;
        String::from_utf8(raw).map_err(|e| OracleError(format!("{}: {e}", path.display())))
    } else {
        String::from_utf8(bytes).map_err(|e| OracleError(format!("{}: {e}", path.display())))
    }
}

// ---- numbers -------------------------------------------------------------------------------

/// A number of any oracle JSON: a JSON number, `{"$float": "inf"|"-inf"|"nan"|"-0"}`
/// (pob-state, pob-dump) or `{"$num": "inf"|…}` (pob-oracle outputs).
#[must_use]
pub fn decode_num(v: &J) -> Option<f64> {
    match v {
        J::Number(n) => n.as_f64(),
        J::Object(m) if m.len() == 1 => {
            let s = m.get("$float").or_else(|| m.get("$num"))?.as_str()?;
            match s {
                "inf" => Some(f64::INFINITY),
                "-inf" => Some(f64::NEG_INFINITY),
                "nan" => Some(f64::NAN),
                "-0" => Some(-0.0),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Members of an encoded table that are never data.
pub const MARKER_KEYS: &[&str] = &[
    "$ref",
    "$data",
    "$node",
    "$treeNode",
    "$object",
    "$class",
    "$env",
    "$foreign",
    "$fn",
    "$float",
    "$num",
    "$bytes",
    "$cycle",
];

/// A marker object (compared as a whole, never descended).
#[must_use]
pub fn is_marker(m: &Map<String, J>) -> bool {
    if m.contains_key("$fn") {
        return true;
    }
    // `$num` is a table member (numeric keys) unless it holds a string
    // (pob-oracle outputs' non-finite number marker)
    m.len() == 1
        && m.iter().next().is_some_and(|(k, v)| {
            MARKER_KEYS.contains(&k.as_str()) && (k != "$num" || v.is_string())
        })
}

// ---- paths -----------------------------------------------------------------------------------

/// A Lua-like path step for a string key (crates/pob-dump/FORMAT.md):
/// `.name` for identifiers, `["…"]` otherwise.
#[must_use]
pub fn step_key(base: &str, key: &str) -> String {
    let ident = key
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if ident {
        if base.is_empty() {
            key.to_owned()
        } else {
            format!("{base}.{key}")
        }
    } else {
        format!("{base}[{}]", J::String(key.to_owned()))
    }
}

/// `base[i]` for sequence elements (1-based) and numeric keys.
#[must_use]
pub fn step_index(base: &str, key: &str) -> String {
    format!("{base}[{key}]")
}

/// The data member name of an encoded key (`$$x` -> `$x`), or `None` for
/// a reserved `$` member.
#[must_use]
pub fn unescape_key(k: &str) -> Option<&str> {
    if let Some(rest) = k.strip_prefix('$') {
        if rest.starts_with('$') {
            return Some(rest);
        }
        return None;
    }
    Some(k)
}

/// The children of an encoded value as (path, value), in encoding order:
/// string members, `$seq`, `$num`, `$keys` (`[@<key json>]`), `$meta`
/// (`<meta>`), plus a store's `mods` and an item's fields.
pub fn children<'a>(base: &str, v: &'a J) -> Vec<(String, &'a J)> {
    let mut out = Vec::new();
    match v {
        J::Array(a) => {
            for (i, x) in a.iter().enumerate() {
                out.push((step_index(base, &(i + 1).to_string()), x));
            }
        }
        J::Object(m) if !is_marker(m) => {
            for (k, x) in m {
                if let Some(name) = unescape_key(k) {
                    out.push((step_key(base, name), x));
                } else if matches!(k.as_str(), "$item" | "$store" | "$uuid") {
                    // data-carrying reserved members
                    out.push((format!("{base}.{k}"), x));
                }
            }
            if let Some(J::Array(seq)) = m.get("$seq") {
                for (i, x) in seq.iter().enumerate() {
                    out.push((step_index(base, &(i + 1).to_string()), x));
                }
            }
            if let Some(J::Object(num)) = m.get("$num") {
                for (k, x) in num {
                    out.push((step_index(base, k), x));
                }
            }
            if let Some(J::Array(keys)) = m.get("$keys") {
                for (i, pair) in keys.iter().enumerate() {
                    if let J::Array(kv) = pair {
                        if kv.len() == 2 {
                            out.push((format!("{base}[@{}:key]", i + 1), &kv[0]));
                            out.push((format!("{base}[@{}]", i + 1), &kv[1]));
                        }
                    }
                }
            }
            if let Some(meta) = m.get("$meta") {
                out.push((format!("{base}<meta>"), meta));
            }
        }
        _ => {}
    }
    out
}

/// Replace the integer identity graph by path references: drops every
/// `"$id"` member and rewrites `{"$ref": n}` to `{"$ref": "<path>"}`, where
/// the path is that of the table's first (full) occurrence.
#[must_use]
pub fn resolve_ids(root: &J) -> J {
    fn collect(path: &str, v: &J, ids: &mut BTreeMap<u64, String>) {
        if let J::Object(m) = v {
            if let Some(id) = m.get("$id").and_then(J::as_u64) {
                ids.entry(id).or_insert_with(|| path.to_owned());
            }
            if is_marker(m) {
                return;
            }
        }
        for (p, c) in children(path, v) {
            collect(&p, c, ids);
        }
    }
    fn rewrite(v: &J, ids: &BTreeMap<u64, String>) -> J {
        match v {
            J::Array(a) => J::Array(a.iter().map(|x| rewrite(x, ids)).collect()),
            J::Object(m) => {
                if m.len() == 1 {
                    if let Some(n) = m.get("$ref").and_then(J::as_u64) {
                        let p = ids.get(&n).cloned().unwrap_or_else(|| format!("?{n}"));
                        let mut o = Map::new();
                        o.insert("$ref".into(), J::String(p));
                        return J::Object(o);
                    }
                }
                J::Object(
                    m.iter()
                        .filter(|(k, _)| k.as_str() != "$id")
                        .map(|(k, x)| (k.clone(), rewrite(x, ids)))
                        .collect(),
                )
            }
            other => other.clone(),
        }
    }
    let mut ids = BTreeMap::new();
    collect("", root, &mut ids);
    rewrite(root, &ids)
}

/// Every leaf of an encoded (normally resolved) tree by Lua-like path.
/// Markers, scalars and empty tables are leaves.
#[must_use]
pub fn flatten_state(root: &J) -> BTreeMap<String, J> {
    fn walk(path: &str, v: &J, out: &mut BTreeMap<String, J>) {
        let kids = children(path, v);
        if kids.is_empty() {
            out.insert(path.to_owned(), v.clone());
            return;
        }
        for (p, c) in kids {
            walk(&p, c, out);
        }
    }
    let mut out = BTreeMap::new();
    walk("", root, &mut out);
    out
}

/// Look up a Lua-like path (as produced by [`children`]) in a tree.
#[must_use]
pub fn get_path<'a>(root: &'a J, path: &str) -> Option<&'a J> {
    fn find<'a>(base: &str, v: &'a J, path: &str) -> Option<&'a J> {
        if base == path {
            return Some(v);
        }
        for (p, c) in children(base, v) {
            if path == p {
                return Some(c);
            }
            if path.starts_with(&p)
                && matches!(path.as_bytes().get(p.len()), Some(b'.' | b'[' | b'<'))
            {
                return find(&p, c, path);
            }
        }
        None
    }
    find("", root, path)
}

// ---- pob-state/1 ---------------------------------------------------------------------------

/// One probe hit (`<outDir>/state/<hit>.json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateDump {
    /// The dump's env identity (`env<k>`, numbered in creation order).
    #[serde(rename = "$env")]
    pub env_id: Option<String>,
    pub build: String,
    pub format: String,
    /// `<probe>#<n>`.
    pub hit: String,
    /// `line`, `in` or `out`.
    pub kind: String,
    pub n: u64,
    /// Probe id (`perform:in`, a stages.toml id, …).
    pub probe: String,
    /// The id of the probe process that wrote the dump (absent in dumps made before ORC-X2).
    #[serde(default)]
    pub run: Option<String>,
    /// `{ "env", "locals", "globalCache" }` as one identity graph.
    pub root: J,
    /// Hit ids of the function probes enclosing this one, outermost first.
    #[serde(default, deserialize_with = "de_list")]
    pub stack: Vec<String>,
}

impl StateDump {
    pub fn from_json(s: &str) -> Result<Self> {
        let d: Self =
            serde_json::from_str(s).map_err(|e| OracleError(format!("pob-state JSON: {e}")))?;
        if d.format != STATE_FORMAT {
            return Err(OracleError(format!("unexpected format {}", d.format)));
        }
        Ok(d)
    }

    pub fn load(path: &Path) -> Result<Self> {
        Self::from_json(&read_text(path)?)
            .map_err(|e| OracleError(format!("{}: {}", path.display(), e.0)))
    }

    #[must_use]
    pub fn env(&self) -> Option<&J> {
        self.root.get("env").filter(|v| !v.is_null())
    }

    #[must_use]
    pub fn locals(&self) -> Option<&J> {
        self.root.get("locals")
    }

    #[must_use]
    pub fn global_cache(&self) -> Option<&J> {
        self.root.get("globalCache")
    }

    /// The root with path references instead of integer ids.
    #[must_use]
    pub fn resolved(&self) -> J {
        resolve_ids(&self.root)
    }
}

/// A mod store of a dump: `{"$store": "ModDB"|"ModList", "mods": …}`.
#[must_use]
pub fn store_class(v: &J) -> Option<&str> {
    v.as_object()?.get("$store")?.as_str()
}

// ---- pob-hits/1 --------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DumpEntry {
    pub hit: String,
    /// Relative to the run directory (`state/<hit>.json`).
    pub file: String,
    #[serde(default, deserialize_with = "de_list")]
    pub stack: Vec<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub env: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HitsIndex {
    pub format: String,
    pub build: String,
    /// Hits per probe id (`perform:in` -> 12).
    #[serde(default, deserialize_with = "de_map")]
    pub counts: BTreeMap<String, u64>,
    #[serde(default, deserialize_with = "de_list")]
    pub dumps: Vec<DumpEntry>,
    /// Selected hits not dumped because of `maxDumps`.
    #[serde(default)]
    pub skipped: u64,
    /// Envs registered (created by initEnv) in the run.
    #[serde(default)]
    pub envs: u64,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub main_env: Option<String>,
    #[serde(default, deserialize_with = "de_list")]
    pub select: Vec<String>,
}

impl HitsIndex {
    pub fn load(path: &Path) -> Result<Self> {
        serde_json::from_str(&read_text(path)?)
            .map_err(|e| OracleError(format!("{}: {e}", path.display())))
    }
}

// ---- pob-provenance/1 ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Provenance {
    #[serde(default)]
    pub format: String,
    #[serde(default)]
    pub build: String,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub env: Option<String>,
    /// `env.player.output`: flattened key -> last writer `file:line`.
    #[serde(default, deserialize_with = "de_map")]
    pub output: BTreeMap<String, String>,
    #[serde(default, deserialize_with = "de_map")]
    pub breakdown: BTreeMap<String, String>,
    #[serde(default, deserialize_with = "de_map")]
    pub enemy: BTreeMap<String, String>,
    #[serde(default, deserialize_with = "de_map")]
    pub minion: BTreeMap<String, String>,
    #[serde(default, deserialize_with = "de_map")]
    pub minion_breakdown: BTreeMap<String, String>,
}

impl Provenance {
    pub fn load(path: &Path) -> Result<Self> {
        serde_json::from_str(&read_text(path)?)
            .map_err(|e| OracleError(format!("{}: {e}", path.display())))
    }

    /// The writer of a flattened output key (as [`OracleOutput::flat`]
    /// names them): its own entry, else the nearest ancestor's (the code
    /// that stored the containing table). `Minion.` keys use the minion map.
    #[must_use]
    pub fn writer(&self, key: &str) -> Option<&str> {
        let (map, key) = match key.strip_prefix("Minion.") {
            Some(k) if !self.minion.is_empty() => (&self.minion, k),
            _ => (&self.output, key),
        };
        let mut k = key;
        loop {
            if let Some(w) = map.get(k) {
                return Some(w.as_str());
            }
            k = &k[..k.rfind('.')?];
        }
    }

    /// Whether `key` itself (not an ancestor) has a recorded writer.
    #[must_use]
    pub fn direct(&self, key: &str) -> bool {
        match key.strip_prefix("Minion.") {
            Some(k) if !self.minion.is_empty() => self.minion.contains_key(k),
            _ => self.output.contains_key(key),
        }
    }
}

/// How much of an output tree has a known writer.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Coverage {
    /// Leaf keys of the outputs (player + `Minion.`).
    pub keys: usize,
    /// Leaves with a writer (own or inherited from the containing key).
    pub covered: usize,
    /// Top-level keys of the proxied tables (output, MainHand, OffHand,
    /// Minion) with a writer recorded on the key itself.
    pub proxied_keys: usize,
    pub proxied_covered: usize,
    pub uncovered: Vec<String>,
}

impl Coverage {
    #[must_use]
    pub fn ratio(&self) -> f64 {
        if self.keys == 0 {
            1.0
        } else {
            self.covered as f64 / self.keys as f64
        }
    }
}

/// Provenance coverage of a run's outputs.
#[must_use]
pub fn coverage(outputs: &OracleOutput, prov: &Provenance) -> Coverage {
    let mut c = Coverage::default();
    for key in outputs.flat().keys() {
        c.keys += 1;
        if prov.writer(key).is_some() {
            c.covered += 1;
        } else if c.uncovered.len() < 50 {
            c.uncovered.push(key.clone());
        }
    }
    // keys stored directly in a proxied table
    let mut proxied = |tree: &Map<String, J>, prefix: &str| {
        for (k, v) in tree {
            let key = format!("{prefix}{k}");
            let nested = matches!(k.as_str(), "MainHand" | "OffHand" | "Minion") && v.is_object();
            if nested {
                if let J::Object(m) = v {
                    for k2 in m.keys() {
                        c.proxied_keys += 1;
                        if prov.direct(&format!("{key}.{k2}")) {
                            c.proxied_covered += 1;
                        }
                    }
                }
            } else {
                c.proxied_keys += 1;
                if prov.direct(&key) {
                    c.proxied_covered += 1;
                }
            }
        }
    };
    proxied(&outputs.player, "");
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_and_flatten() {
        let root: J = serde_json::from_str(
            r#"{"env":{"$id":1,"a":{"x":1,"$seq":[true]},"self":{"$ref":1}},
                "locals":{"e":{"$ref":1},"$$k":2,"n":{"$num":{"0":3}},"f":{"$float":"inf"}}}"#,
        )
        .unwrap();
        let r = resolve_ids(&root);
        assert_eq!(r["env"]["self"], serde_json::json!({"$ref": "env"}));
        assert_eq!(r["locals"]["e"], serde_json::json!({"$ref": "env"}));
        assert!(r["env"].get("$id").is_none());
        let flat = flatten_state(&r);
        assert_eq!(flat["env.a.x"], 1);
        assert_eq!(flat["env.a[1]"], true);
        assert_eq!(flat["locals[\"$k\"]"], 2);
        assert_eq!(flat["locals.n[0]"], 3);
        assert_eq!(decode_num(&flat["locals.f"]), Some(f64::INFINITY));
        assert_eq!(get_path(&r, "env.a[1]"), Some(&J::Bool(true)));
    }

    #[test]
    fn provenance_inherits_from_ancestors() {
        let mut p = Provenance::default();
        p.output
            .insert("SkillDPS".into(), "Modules/Calcs.lua:10".into());
        p.output
            .insert("MainHand.Speed".into(), "Modules/CalcOffence.lua:20".into());
        assert_eq!(p.writer("SkillDPS.1.dps"), Some("Modules/Calcs.lua:10"));
        assert_eq!(
            p.writer("MainHand.Speed"),
            Some("Modules/CalcOffence.lua:20")
        );
        assert_eq!(p.writer("Life"), None);
        assert!(!p.direct("SkillDPS.1"));
    }
}
