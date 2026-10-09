//! PoB nondeterminism (calc-architecture.md §9.5, D8).
//!
//! LuaJIT seeds its string hash per process, so `pairs()` over string keys
//! visits keys in a different order in every PoB process, and anything PoB
//! derives from that order (list order, mod order within a name, the order
//! float sums are accumulated in, which skill a cache entry belongs to, …)
//! can differ between two runs of the same build. The oracle therefore runs
//! every build twice (two processes) and records whatever differs in
//! `.parity/nondet/<build>.json` ([`NondetRecord`]): the values PoB produced
//! at each differing path, and the lists whose order differs while their
//! contents agree. A build flagged by the first pair runs a third time and
//! the third run's differences are added. The diff ([`crate::diff`])
//! accepts any recorded value and compares recorded lists as multisets.
//!
//! Paths: outputs use the flattened output keys of `pob_calc_diff`
//! (`MainHand.Speed`, `SkillDPS.1.dps`, `Minion.Life`); states use the
//! Lua-like paths of [`crate::schema::children`] over the resolved dump.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value as J;

use crate::oracle::{OracleError, Result};
use crate::schema::{children, is_marker, read_text, resolve_ids, HitsIndex, StateDump};

pub const NONDET_FORMAT: &str = "pob-nondet/1";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NondetRecord {
    pub format: String,
    pub build: String,
    /// Processes compared.
    pub runs: usize,
    /// Output key -> every distinct value PoB produced there.
    #[serde(default)]
    pub outputs: BTreeMap<String, Vec<J>>,
    /// Output lists whose element order differs between runs.
    #[serde(default)]
    pub output_orders: BTreeSet<String>,
    /// Hit -> state path -> values.
    #[serde(default)]
    pub states: BTreeMap<String, BTreeMap<String, Vec<J>>>,
    /// Hit -> state lists (and mod lists) whose order differs.
    #[serde(default)]
    pub state_orders: BTreeMap<String, BTreeSet<String>>,
    /// Probe id -> hit count per run, for probes whose count differs.
    #[serde(default)]
    pub hit_counts: BTreeMap<String, Vec<u64>>,
    /// Hits dumped by some runs only.
    #[serde(default)]
    pub dumps_missing: BTreeSet<String>,
}

impl NondetRecord {
    #[must_use]
    pub fn new(build: &str) -> Self {
        Self {
            format: NONDET_FORMAT.to_owned(),
            build: build.to_owned(),
            ..Self::default()
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Number of recorded entries (values + orders + counts + missing).
    #[must_use]
    pub fn len(&self) -> usize {
        self.outputs.len()
            + self.output_orders.len()
            + self.states.values().map(BTreeMap::len).sum::<usize>()
            + self.state_orders.values().map(BTreeSet::len).sum::<usize>()
            + self.hit_counts.len()
            + self.dumps_missing.len()
    }

    /// Every entry as a flat label (`outputs:<path>`, `state:<hit>:<path>`,
    /// `order:…`, `count:<id>`, `dump:<hit>`).
    #[must_use]
    pub fn labels(&self) -> BTreeSet<String> {
        let mut s = BTreeSet::new();
        for k in self.outputs.keys() {
            s.insert(format!("outputs:{k}"));
        }
        for k in &self.output_orders {
            s.insert(format!("order:outputs:{k}"));
        }
        for (h, m) in &self.states {
            for k in m.keys() {
                s.insert(format!("state:{h}:{k}"));
            }
        }
        for (h, m) in &self.state_orders {
            for k in m {
                s.insert(format!("order:{h}:{k}"));
            }
        }
        for k in self.hit_counts.keys() {
            s.insert(format!("count:{k}"));
        }
        for k in &self.dumps_missing {
            s.insert(format!("dump:{k}"));
        }
        s
    }

    /// Entries of `other` that this record does not cover (a value path
    /// or order path recorded here covers any entry at or below it). With
    /// `structural`, paths are compared after [`generalize`]: PoB
    /// nondeterminism seen at one list element (`activeSkillList[3]…`) is
    /// taken to hold for its siblings.
    #[must_use]
    pub fn uncovered(&self, other: &NondetRecord, structural: bool) -> Vec<String> {
        let norm = |s: &str| -> String {
            let s = s.strip_prefix("order:").unwrap_or(s);
            if structural {
                generalize(s)
            } else {
                s.to_owned()
            }
        };
        let mine: BTreeSet<String> = self.labels().iter().map(|m| norm(m)).collect();
        let covers = |label: &str| {
            let l = norm(label);
            mine.iter().any(|m| {
                l == *m
                    || (l.starts_with(m.as_str())
                        && matches!(l.as_bytes().get(m.len()), Some(b'.' | b'[' | b'<')))
            })
        };
        other.labels().into_iter().filter(|l| !covers(l)).collect()
    }

    pub fn merge(&mut self, other: &NondetRecord) {
        let add = |dst: &mut Vec<J>, src: &[J]| {
            for v in src {
                if !dst.contains(v) {
                    dst.push(v.clone());
                }
            }
        };
        for (k, v) in &other.outputs {
            add(self.outputs.entry(k.clone()).or_default(), v);
        }
        self.output_orders
            .extend(other.output_orders.iter().cloned());
        for (h, m) in &other.states {
            let dst = self.states.entry(h.clone()).or_default();
            for (k, v) in m {
                add(dst.entry(k.clone()).or_default(), v);
            }
        }
        for (h, m) in &other.state_orders {
            self.state_orders
                .entry(h.clone())
                .or_default()
                .extend(m.iter().cloned());
        }
        for (k, v) in &other.hit_counts {
            let dst = self.hit_counts.entry(k.clone()).or_default();
            for c in v {
                if !dst.contains(c) {
                    dst.push(*c);
                }
            }
        }
        self.dumps_missing
            .extend(other.dumps_missing.iter().cloned());
        self.runs = self.runs.max(other.runs);
    }

    pub fn load(path: &Path) -> Result<Self> {
        serde_json::from_str(&read_text(path)?)
            .map_err(|e| OracleError(format!("{}: {e}", path.display())))
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d).map_err(|e| OracleError(format!("{}: {e}", d.display())))?;
        }
        let s = serde_json::to_string_pretty(self).map_err(|e| OracleError(e.to_string()))?;
        let tmp = path.with_extension("json.part");
        std::fs::write(&tmp, s).map_err(|e| OracleError(format!("{}: {e}", tmp.display())))?;
        std::fs::rename(&tmp, path).map_err(|e| OracleError(format!("{}: {e}", path.display())))
    }
}

/// A path with every list index replaced by `*` (`a[3].b` -> `a[*].b`,
/// output keys `SkillDPS.2.dps` -> `SkillDPS.*.dps`); the hit number of a
/// state label (`perform:in#3`) is kept.
#[must_use]
pub fn generalize(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    let b = path.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if (c == b'[' || c == b'.') && b.get(i + 1).is_some_and(u8::is_ascii_digit) {
            let mut j = i + 1;
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            let closes = if c == b'[' {
                b.get(j) == Some(&b']')
            } else {
                j == b.len() || b[j] == b'.'
            };
            if closes {
                out.push(c as char);
                out.push('*');
                i = j;
                continue;
            }
        }
        out.push(c as char);
        i += 1;
    }
    out
}

// ---- comparing trees across runs ------------------------------------------------------------------

type Kids = for<'a> fn(&str, &'a J) -> Vec<(String, &'a J)>;

/// Children of an outputs tree with the flattened-key paths of
/// `oracle::flatten` (`a.b`, arrays 1-based `a.1`).
fn output_children<'a>(base: &str, v: &'a J) -> Vec<(String, &'a J)> {
    let key = |k: &str| {
        if base.is_empty() {
            k.to_owned()
        } else {
            format!("{base}.{k}")
        }
    };
    match v {
        J::Array(a) => a
            .iter()
            .enumerate()
            .map(|(i, x)| (key(&(i + 1).to_string()), x))
            .collect(),
        J::Object(m) if !(m.len() == 1 && m.keys().next().is_some_and(|k| k.starts_with('$'))) => {
            m.iter().map(|(k, x)| (key(k), x)).collect()
        }
        _ => Vec::new(),
    }
}

fn state_children<'a>(base: &str, v: &'a J) -> Vec<(String, &'a J)> {
    children(base, v)
}

fn canonical(v: &J) -> String {
    v.to_string()
}

/// [`canonical`] with every list (arrays, `$seq`) sorted, recursively:
/// equal for values that differ only in the order of nested lists.
pub(crate) fn deep_canonical(v: &J) -> String {
    match v {
        J::Array(a) => {
            let mut items: Vec<String> = a.iter().map(deep_canonical).collect();
            items.sort();
            format!("[{}]", items.join(","))
        }
        J::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            let items: Vec<String> = keys
                .into_iter()
                .map(|k| format!("{}:{}", J::String(k.clone()), deep_canonical(&m[k])))
                .collect();
            format!("{{{}}}", items.join(","))
        }
        other => other.to_string(),
    }
}

/// Compare the values several runs produced at `path`; record differences.
fn compare(
    path: &str,
    vals: &[Option<&J>],
    kids: Kids,
    values: &mut BTreeMap<String, Vec<J>>,
    orders: &mut BTreeSet<String>,
) {
    let first = vals[0];
    if vals.iter().all(|v| *v == first) {
        return;
    }
    let structured = |v: Option<&J>| match v {
        Some(J::Array(_)) => true,
        Some(J::Object(m)) => !is_marker(m),
        _ => false,
    };
    if vals.iter().all(|v| structured(*v)) {
        let lists: Vec<Vec<(String, &J)>> = vals
            .iter()
            .map(|v| kids(path, v.unwrap_or(&J::Null)))
            .collect();
        // same elements in another order: a list (a sequence 1..n, the mods
        // of a store under one name, or the `$keys` entries of a table-keyed
        // map, whose order is that of keys PoB cannot sort). Numeric keys
        // outside 1..n are written sorted, so they never form an order.
        let is_seq = |l: &[(String, &J)]| {
            let positional = l.iter().enumerate().all(|(i, (p, _))| {
                let n = (i + 1).to_string();
                p.strip_prefix(path).is_some_and(|rest| {
                    rest == format!("[{n}]")
                        || rest == format!(".{n}")
                        || (path.is_empty() && rest == n)
                })
            });
            positional
                || l.iter()
                    .all(|(p, _)| p.strip_prefix(path).is_some_and(|r| r.starts_with("[@")))
        };
        if lists.iter().all(|l| is_seq(l)) && !lists[0].is_empty() {
            let bags: Vec<Vec<String>> = lists
                .iter()
                .map(|l| {
                    let mut b: Vec<String> = l.iter().map(|(_, x)| canonical(x)).collect();
                    b.sort();
                    b
                })
                .collect();
            if bags.iter().all(|b| *b == bags[0]) {
                orders.insert(path.to_owned());
                return;
            }
            // the same elements up to the order of lists nested in them (a
            // buff list whose buffs' mod lists are reordered too): record the
            // order here and compare each element with its counterpart
            let positional = lists
                .iter()
                .all(|l| l.iter().all(|(p, _)| !p.contains("[@")));
            let deep: Vec<Vec<String>> = lists
                .iter()
                .map(|l| l.iter().map(|(_, x)| deep_canonical(x)).collect())
                .collect();
            let sorted = |d: &Vec<String>| {
                let mut s = d.clone();
                s.sort();
                s
            };
            if positional && deep.iter().all(|d| sorted(d) == sorted(&deep[0])) {
                orders.insert(path.to_owned());
                let mut used: Vec<Vec<bool>> = lists.iter().map(|l| vec![false; l.len()]).collect();
                for (i, (p, x)) in lists[0].iter().enumerate() {
                    let mut group: Vec<Option<&J>> = vec![Some(*x)];
                    for r in 1..lists.len() {
                        let j =
                            (0..lists[r].len()).find(|&j| !used[r][j] && deep[r][j] == deep[0][i]);
                        group.push(j.map(|j| {
                            used[r][j] = true;
                            lists[r][j].1
                        }));
                    }
                    compare(p, &group, kids, values, orders);
                }
                return;
            }
        }
        let mut paths: BTreeSet<&str> = BTreeSet::new();
        for l in &lists {
            for (p, _) in l {
                paths.insert(p.as_str());
            }
        }
        let maps: Vec<BTreeMap<&str, &J>> = lists
            .iter()
            .map(|l| l.iter().map(|(p, x)| (p.as_str(), *x)).collect())
            .collect();
        for p in paths {
            let sub: Vec<Option<&J>> = maps.iter().map(|m| m.get(p).copied()).collect();
            compare(p, &sub, kids, values, orders);
        }
        return;
    }
    let entry = values.entry(path.to_owned()).or_default();
    for v in vals {
        let v = v.cloned().unwrap_or(J::Null);
        if !entry.contains(&v) {
            entry.push(v);
        }
    }
}

/// A probe run directory: `outputs.json`, `hits.json`, `state/*.json`.
fn load_json(path: &Path) -> Result<J> {
    serde_json::from_str(&read_text(path)?)
        .map_err(|e| OracleError(format!("{}: {e}", path.display())))
}

/// Compare probe runs of one build (each a `pob_oracle probe --out-dir`).
pub fn compare_runs(build: &str, runs: &[&Path]) -> Result<NondetRecord> {
    if runs.len() < 2 {
        return Err(OracleError("need at least two runs".into()));
    }
    let mut rec = NondetRecord::new(build);
    rec.runs = runs.len();
    // outputs
    let outs: Vec<J> = runs
        .iter()
        .map(|r| load_json(&r.join("outputs.json")))
        .collect::<Result<_>>()?;
    let trees: Vec<J> = outs
        .iter()
        .map(|o| {
            let mut m = o.get("player").cloned().unwrap_or(J::Null);
            if let (J::Object(pm), Some(J::Object(mm))) = (&mut m, o.get("minion")) {
                pm.insert("Minion".into(), J::Object(mm.clone()));
            }
            m
        })
        .collect();
    let refs: Vec<Option<&J>> = trees.iter().map(Some).collect();
    compare(
        "",
        &refs,
        output_children,
        &mut rec.outputs,
        &mut rec.output_orders,
    );
    // hit counts and dumps
    let hits: Vec<HitsIndex> = runs
        .iter()
        .map(|r| HitsIndex::load(&r.join("hits.json")))
        .collect::<Result<_>>()?;
    let ids: BTreeSet<&String> = hits.iter().flat_map(|h| h.counts.keys()).collect();
    for id in ids {
        let counts: Vec<u64> = hits
            .iter()
            .map(|h| h.counts.get(id).copied().unwrap_or(0))
            .collect();
        if counts.iter().any(|c| *c != counts[0]) {
            rec.hit_counts.insert(id.clone(), counts);
        }
    }
    let dumped: Vec<BTreeMap<&str, &str>> = hits
        .iter()
        .map(|h| {
            h.dumps
                .iter()
                .map(|d| (d.hit.as_str(), d.file.as_str()))
                .collect()
        })
        .collect();
    let all_hits: BTreeSet<&str> = dumped.iter().flat_map(|m| m.keys().copied()).collect();
    for hit in all_hits {
        let files: Vec<Option<&&str>> = dumped.iter().map(|m| m.get(hit)).collect();
        if files.iter().any(Option::is_none) {
            rec.dumps_missing.insert(hit.to_owned());
            continue;
        }
        let docs: Vec<J> = runs
            .iter()
            .zip(&files)
            .map(|(r, f)| {
                let d = StateDump::load(&r.join(f.unwrap_or(&"")))?;
                let mut head = serde_json::Map::new();
                head.insert(
                    "stack".into(),
                    serde_json::to_value(&d.stack).unwrap_or(J::Null),
                );
                head.insert(
                    "$env".into(),
                    serde_json::to_value(&d.env_id).unwrap_or(J::Null),
                );
                head.insert("root".into(), resolve_ids(&d.root));
                Ok(J::Object(head))
            })
            .collect::<Result<_>>()?;
        let refs: Vec<Option<&J>> = docs.iter().map(Some).collect();
        let mut values = BTreeMap::new();
        let mut orders = BTreeSet::new();
        compare("", &refs, state_children, &mut values, &mut orders);
        if !values.is_empty() {
            rec.states.insert(hit.to_owned(), values);
        }
        if !orders.is_empty() {
            rec.state_orders.insert(hit.to_owned(), orders);
        }
    }
    Ok(rec)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn records_values_and_orders() {
        let a = json!({"x": [1, 2, 3], "y": {"z": 1, "w": "s"}});
        let b = json!({"x": [3, 1, 2], "y": {"z": 2, "w": "s"}});
        let mut values = BTreeMap::new();
        let mut orders = BTreeSet::new();
        compare(
            "",
            &[Some(&a), Some(&b)],
            output_children,
            &mut values,
            &mut orders,
        );
        assert_eq!(orders.into_iter().collect::<Vec<_>>(), vec!["x".to_owned()]);
        assert_eq!(values.get("y.z"), Some(&vec![json!(1), json!(2)]));
        assert_eq!(values.len(), 1);
    }

    #[test]
    fn nested_orders_pair_elements() {
        // two buffs swapped, and the first buff's mod list reordered too
        let a = json!({"buffs": [{"n": "A", "mods": [1, 2]}, {"n": "B", "mods": [3]}]});
        let b = json!({"buffs": [{"n": "B", "mods": [3]}, {"n": "A", "mods": [2, 1]}]});
        let mut values = BTreeMap::new();
        let mut orders = BTreeSet::new();
        compare(
            "",
            &[Some(&a), Some(&b)],
            output_children,
            &mut values,
            &mut orders,
        );
        assert!(values.is_empty(), "{values:?}");
        assert_eq!(
            orders.into_iter().collect::<Vec<_>>(),
            vec!["buffs".to_owned(), "buffs.1.mods".to_owned()]
        );
        // a real value difference inside a reordered list stays a value
        let c = json!({"buffs": [{"n": "B", "mods": [4]}, {"n": "A", "mods": [2, 1]}]});
        let mut values = BTreeMap::new();
        let mut orders = BTreeSet::new();
        compare(
            "",
            &[Some(&a), Some(&c)],
            output_children,
            &mut values,
            &mut orders,
        );
        assert!(!values.is_empty());
    }

    #[test]
    fn uncovered_entries() {
        let mut a = NondetRecord::new("b");
        a.outputs.insert("SkillDPS".into(), vec![]);
        let mut b = NondetRecord::new("b");
        b.outputs.insert("SkillDPS.1.dps".into(), vec![]);
        b.outputs.insert("Life".into(), vec![]);
        assert_eq!(a.uncovered(&b, false), vec!["outputs:Life".to_owned()]);
        let mut c = NondetRecord::new("b");
        c.state_orders
            .entry("h#1".into())
            .or_default()
            .insert("env.list[2].mods".into());
        let mut d = NondetRecord::new("b");
        d.state_orders
            .entry("h#1".into())
            .or_default()
            .insert("env.list[7].mods".into());
        assert_eq!(c.uncovered(&d, false).len(), 1);
        assert!(c.uncovered(&d, true).is_empty());
        assert_eq!(generalize("SkillDPS.12.dps"), "SkillDPS.*.dps");
        assert_eq!(generalize("x[3].y[@2]"), "x[*].y[@2]");
    }
}
