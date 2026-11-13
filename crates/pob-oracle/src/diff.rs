//! Comparing PoB (the oracle) against the Rust calc (calc-architecture.md
//! §9.2 "Diff").
//!
//! * Numbers agree when `|a-b| <= 1e-9 * max(1, |a|)` (`a` = PoB); booleans
//!   and strings must be identical; markers compare as whole values. A ±0
//!   sign mismatch is therefore tolerated: it is a host artefact of a
//!   dual-number (ARM64) oracle LuaJIT, never a port bug (CCR-0001/0002).
//!   Values derived from the sign (`1/x` = ±inf, a formatted `"-0"`) are
//!   not; record those in `known_diffs.toml` citing CCR-0002.
//! * Mod stores (`$store`) compare as a multiset of mods per name (ModList:
//!   over the whole list); when the multisets agree but the order does not,
//!   the mismatch is reported as `LUA-ORDER`.
//! * Lists compare in order, except at paths in the nondeterministic set
//!   (§9.5, [`crate::nondet`]), which compare as multisets; a value at a
//!   nondeterministic path is accepted when it equals any value PoB
//!   produced.
//! * [`calc_diff_doc`] writes `{build, pob, ours, …}` with flattened
//!   outputs, the shape `scripts/parity/report.py` reads.
//! * [`unit_ranges`] reads a unit's Lua ranges from the unit table of
//!   calc-architecture.md, so `pob_calc_diff --unit <id>` can keep only the
//!   mismatches whose last writer (write provenance) is in the unit.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value as J};

use crate::oracle::{flatten, Leaf, OracleError, Result};
use crate::schema::{children, decode_num, is_marker, read_text, store_class, Provenance, INPUT_KEYS};

fn is_input_key(k: &str) -> bool {
    INPUT_KEYS.contains(&k)
}

/// Markers (and other unstructured values) compare whole, except for the injection inputs
/// they carry (a `$fn` marker's `$up`, FUP3).
fn whole_eq(a: &J, b: &J) -> bool {
    match (a, b) {
        (J::Object(x), J::Object(y)) => {
            x.len() - x.keys().filter(|k| is_input_key(k)).count()
                == y.len() - y.keys().filter(|k| is_input_key(k)).count()
                && x.iter().filter(|(k, _)| !is_input_key(k)).all(|(k, v)| y.get(k) == Some(v))
        }
        _ => a == b,
    }
}

/// §9.2 numeric tolerance (relative, with an absolute floor of 1).
pub const REL_TOL: f64 = 1e-9;

#[must_use]
pub fn num_close(pob: f64, ours: f64, rel: f64) -> bool {
    if pob.is_nan() || ours.is_nan() {
        return pob.is_nan() && ours.is_nan();
    }
    if pob.is_infinite() || ours.is_infinite() {
        return pob == ours;
    }
    (pob - ours).abs() <= rel * pob.abs().max(1.0)
}

/// Leaf equality: numbers within `rel`, anything else identical.
#[must_use]
pub fn leaf_eq(pob: &J, ours: &J, rel: f64) -> bool {
    match (decode_num(pob), decode_num(ours)) {
        (Some(a), Some(b)) => num_close(a, b, rel),
        _ => pob == ours,
    }
}

/// Deep equality with the numeric tolerance (paths ignored, the injection inputs `$at`, `$up`,
/// `$modLines` ignored: [`crate::schema::INPUT_KEYS`]).
#[must_use]
pub fn tree_eq(a: &J, b: &J, rel: f64) -> bool {
    if let (Some(x), Some(y)) = (decode_num(a), decode_num(b)) {
        return num_close(x, y, rel);
    }
    match (a, b) {
        (J::Array(x), J::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| tree_eq(p, q, rel))
        }
        (J::Object(x), J::Object(y)) => {
            let kx: Vec<_> = x.keys().filter(|k| !is_input_key(k)).collect();
            let ky: Vec<_> = y.keys().filter(|k| !is_input_key(k)).collect();
            kx == ky
                && kx
                    .iter()
                    .all(|k| tree_eq(&x[k.as_str()], &y[k.as_str()], rel))
        }
        _ => a == b,
    }
}

/// [`tree_eq`] with every list (arrays, `$seq`, a store's mods per name)
/// compared as a multiset, recursively.
#[must_use]
pub fn tree_eq_unordered(a: &J, b: &J, rel: f64) -> bool {
    if let (Some(x), Some(y)) = (decode_num(a), decode_num(b)) {
        return num_close(x, y, rel);
    }
    let bag_eq = |x: &[J], y: &[J]| {
        let mut used = vec![false; y.len()];
        x.len() == y.len()
            && x.iter().all(|p| {
                (0..y.len())
                    .find(|&i| !used[i] && tree_eq_unordered(p, &y[i], rel))
                    .map(|i| used[i] = true)
                    .is_some()
            })
    };
    match (a, b) {
        (J::Array(x), J::Array(y)) => bag_eq(x, y),
        (J::Object(x), J::Object(y)) => {
            let kx: Vec<_> = x.keys().filter(|k| !is_input_key(k)).collect();
            let ky: Vec<_> = y.keys().filter(|k| !is_input_key(k)).collect();
            kx == ky
                && kx
                    .iter()
                    .all(|k| tree_eq_unordered(&x[k.as_str()], &y[k.as_str()], rel))
        }
        _ => a == b,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MismatchKind {
    /// Values differ.
    #[serde(rename = "value")]
    Value,
    /// PoB has it, ours does not.
    #[serde(rename = "missing")]
    Missing,
    /// Ours has it, PoB does not.
    #[serde(rename = "extra")]
    Extra,
    /// Same multiset of mods (or list elements), different order.
    #[serde(rename = "LUA-ORDER")]
    LuaOrder,
    /// A mod of PoB's store with no equal mod in ours.
    #[serde(rename = "mod-missing")]
    ModMissing,
    /// A mod of our store with no equal mod in PoB's.
    #[serde(rename = "mod-extra")]
    ModExtra,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mismatch {
    pub path: String,
    pub kind: MismatchKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pob: Option<J>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ours: Option<J>,
    /// PoB's last writer / inserting line (`file:line`), when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub writer: Option<String>,
}

/// Diff settings: tolerance and the nondeterministic set of the document.
#[derive(Debug, Clone, Default)]
pub struct DiffOptions<'a> {
    pub rel: f64,
    /// path -> every value PoB produced there.
    pub nondet_values: Option<&'a BTreeMap<String, Vec<J>>>,
    /// paths of lists whose order PoB does not fix.
    pub nondet_orders: Option<&'a BTreeSet<String>>,
}

impl DiffOptions<'_> {
    #[must_use]
    pub fn strict() -> Self {
        Self {
            rel: REL_TOL,
            ..Self::default()
        }
    }

    fn accepted(&self, path: &str, ours: &J) -> bool {
        self.nondet_values
            .and_then(|m| m.get(path))
            .is_some_and(|vals| vals.iter().any(|v| tree_eq(v, ours, self.rel)))
    }

    fn unordered(&self, path: &str) -> bool {
        self.nondet_orders.is_some_and(|s| s.contains(path))
    }
}

/// Greedy multiset matching with tolerance: (unmatched of a, unmatched of b).
fn multiset_unmatched<'a>(a: &'a [J], b: &'a [J], rel: f64) -> (Vec<&'a J>, Vec<&'a J>) {
    let mut used = vec![false; b.len()];
    let mut left = Vec::new();
    for x in a {
        if let Some(i) = (0..b.len()).find(|&i| !used[i] && tree_eq(x, &b[i], rel)) {
            used[i] = true;
        } else {
            left.push(x);
        }
    }
    let right = b
        .iter()
        .zip(&used)
        .filter(|(_, u)| !**u)
        .map(|(y, _)| y)
        .collect();
    (left, right)
}

/// A mod store's mods by name (a ModList's under `""`).
pub(crate) fn store_mods(v: &J) -> BTreeMap<String, Vec<J>> {
    let mut out = BTreeMap::new();
    match v.get("mods") {
        Some(J::Object(m)) => {
            for (k, list) in m {
                let name = crate::schema::unescape_key(k).unwrap_or(k).to_owned();
                out.insert(name, list.as_array().cloned().unwrap_or_default());
            }
        }
        Some(J::Array(list)) => {
            out.insert(String::new(), list.clone());
        }
        _ => {}
    }
    out
}

/// Path of a store's mods under one name (`<store>.mods.Life`,
/// `<store>.mods["Name With Spaces"]`; a ModList's whole list `<store>.mods`):
/// where the diff compares them as a multiset and where the nondet record
/// keeps their orders and accepted alternatives.
#[must_use]
pub fn store_list_path(path: &str, name: &str) -> String {
    let mods = format!("{path}.mods");
    if name.is_empty() {
        mods
    } else {
        crate::schema::step_key(&mods, name)
    }
}

fn mod_at(m: &J) -> Option<String> {
    m.get("$at").and_then(J::as_str).map(str::to_owned)
}

fn diff_store(path: &str, a: &J, b: &J, opts: &DiffOptions<'_>, out: &mut Vec<Mismatch>) {
    let (ca, cb) = (store_class(a), store_class(b));
    if ca != cb {
        out.push(Mismatch {
            path: format!("{path}.$store"),
            kind: MismatchKind::Value,
            pob: ca.map(|s| J::String(s.to_owned())),
            ours: cb.map(|s| J::String(s.to_owned())),
            writer: None,
        });
    }
    let (ma, mb) = (store_mods(a), store_mods(b));
    let names: BTreeSet<&String> = ma.keys().chain(mb.keys()).collect();
    for name in names {
        let empty = Vec::new();
        let la = ma.get(name).unwrap_or(&empty);
        let lb = mb.get(name).unwrap_or(&empty);
        let p = store_list_path(path, name);
        // a list PoB produced in some run (the nondet record keeps each
        // run's list when they differ beyond order; `[]` for an absent name)
        let alternative = opts
            .nondet_values
            .and_then(|m| m.get(&p))
            .is_some_and(|alts| {
                alts.iter().any(|alt| {
                    let alt = alt.as_array().map_or(&[][..], Vec::as_slice);
                    let (unmatched_alt, unmatched_ours) = multiset_unmatched(alt, lb, opts.rel);
                    unmatched_alt.is_empty() && unmatched_ours.is_empty()
                })
            });
        if alternative {
            continue;
        }
        let (left, right) = multiset_unmatched(la, lb, opts.rel);
        if left.is_empty() && right.is_empty() {
            let same_order = la.iter().zip(lb).all(|(x, y)| tree_eq(x, y, opts.rel));
            if !same_order && !opts.unordered(&p) {
                out.push(Mismatch {
                    path: p,
                    kind: MismatchKind::LuaOrder,
                    pob: None,
                    ours: None,
                    writer: None,
                });
            }
            continue;
        }
        for m in left {
            out.push(Mismatch {
                path: p.clone(),
                kind: MismatchKind::ModMissing,
                pob: Some(m.clone()),
                ours: None,
                writer: mod_at(m),
            });
        }
        for m in right {
            out.push(Mismatch {
                path: p.clone(),
                kind: MismatchKind::ModExtra,
                pob: None,
                ours: Some(m.clone()),
                writer: None,
            });
        }
    }
    // the other members (parent, actor, conditions, multipliers, …)
    let strip = |v: &J| -> J {
        let mut m = v.as_object().cloned().unwrap_or_default();
        m.remove("mods");
        m.remove("$store");
        J::Object(m)
    };
    walk(path, &strip(a), &strip(b), opts, out);
}

fn walk(path: &str, a: &J, b: &J, opts: &DiffOptions<'_>, out: &mut Vec<Mismatch>) {
    if opts.accepted(path, b) {
        return;
    }
    if let (Some(x), Some(y)) = (decode_num(a), decode_num(b)) {
        if !num_close(x, y, opts.rel) {
            out.push(Mismatch {
                path: path.to_owned(),
                kind: MismatchKind::Value,
                pob: Some(a.clone()),
                ours: Some(b.clone()),
                writer: None,
            });
        }
        return;
    }
    if store_class(a).is_some() && store_class(b).is_some() {
        return diff_store(path, a, b, opts, out);
    }
    let structured = |v: &J| match v {
        J::Array(_) => true,
        J::Object(m) => !is_marker(m),
        _ => false,
    };
    if !(structured(a) && structured(b)) {
        if !whole_eq(a, b) {
            out.push(Mismatch {
                path: path.to_owned(),
                kind: MismatchKind::Value,
                pob: Some(a.clone()),
                ours: Some(b.clone()),
                writer: None,
            });
        }
        return;
    }
    if opts.unordered(path) {
        let seq = |v: &J| -> Vec<J> {
            children(path, v)
                .into_iter()
                .map(|(_, x)| x.clone())
                .collect()
        };
        let (sa, sb) = (seq(a), seq(b));
        let (left, right) = multiset_unmatched(&sa, &sb, opts.rel);
        // elements that differ only in the order of lists nested in them, when
        // the record has such nested orders below this list (PoB reorders
        // them together with the list; nondet pairs the elements and records
        // the nested orders at run 1's positions)
        let nested = opts.nondet_orders.is_some_and(|s| {
            s.iter().any(|o| {
                o.strip_prefix(path)
                    .is_some_and(|r| r.starts_with('[') || r.starts_with('.'))
            })
        });
        let (left_rest, right_rest) = if nested {
            let mut used = vec![false; right.len()];
            let left_rest = left
                .iter()
                .filter(|x| {
                    match (0..right.len())
                        .find(|&i| !used[i] && tree_eq_unordered(x, right[i], opts.rel))
                    {
                        Some(i) => {
                            used[i] = true;
                            false
                        }
                        None => true,
                    }
                })
                .count();
            (left_rest, used.iter().filter(|u| !**u).count())
        } else {
            (left.len(), right.len())
        };
        if left_rest > 0 || right_rest > 0 {
            out.push(Mismatch {
                path: path.to_owned(),
                kind: MismatchKind::Value,
                pob: Some(a.clone()),
                ours: Some(b.clone()),
                writer: None,
            });
        }
        return;
    }
    let ka: BTreeMap<String, &J> = children(path, a).into_iter().collect();
    let kb: BTreeMap<String, &J> = children(path, b).into_iter().collect();
    for (p, x) in &ka {
        match kb.get(p) {
            Some(y) => walk(p, x, y, opts, out),
            None => out.push(Mismatch {
                path: p.clone(),
                kind: MismatchKind::Missing,
                pob: Some((*x).clone()),
                ours: None,
                writer: None,
            }),
        }
    }
    for (p, y) in &kb {
        if !ka.contains_key(p) {
            out.push(Mismatch {
                path: p.clone(),
                kind: MismatchKind::Extra,
                pob: None,
                ours: Some((*y).clone()),
                writer: None,
            });
        }
    }
}

/// Diff two encoded trees (pob-state roots, resolved with
/// [`crate::schema::resolve_ids`]).
#[must_use]
pub fn diff_trees(pob: &J, ours: &J, opts: &DiffOptions<'_>) -> Vec<Mismatch> {
    let mut out = Vec::new();
    walk("", pob, ours, opts, &mut out);
    out
}

// ---- outputs ------------------------------------------------------------------------------------

/// A flattened output leaf as JSON (non-finite numbers as `{"$num": …}`,
/// which report.py and diff.py decode).
#[must_use]
pub fn leaf_json(l: &Leaf) -> J {
    match l {
        Leaf::Num(n) if n.is_finite() => {
            serde_json::Number::from_f64(*n).map_or(J::Null, J::Number)
        }
        Leaf::Num(n) => {
            let s = if n.is_nan() {
                "nan"
            } else if *n > 0.0 {
                "inf"
            } else {
                "-inf"
            };
            let mut m = Map::new();
            m.insert("$num".into(), J::String(s.into()));
            J::Object(m)
        }
        Leaf::Bool(b) => J::Bool(*b),
        Leaf::Str(s) => J::String(s.clone()),
    }
}

/// An output file as flattened leaves: a pob_oracle `outputs` dump
/// (`player` + `minion` under `Minion.`) or any JSON object of outputs
/// (the same rule as scripts/parity/diff.py).
pub fn load_flat_outputs(path: &Path) -> Result<BTreeMap<String, J>> {
    let doc: J = serde_json::from_str(&read_text(path)?)
        .map_err(|e| OracleError(format!("{}: {e}", path.display())))?;
    Ok(flat_outputs(&doc))
}

#[must_use]
pub fn flat_outputs(doc: &J) -> BTreeMap<String, J> {
    let tree = |v: &J| v.as_object().cloned().unwrap_or_default();
    let mut out: BTreeMap<String, J> = BTreeMap::new();
    if let Some(player @ J::Object(_)) = doc.get("player") {
        for (k, l) in flatten(&tree(player)) {
            out.insert(k, leaf_json(&l));
        }
        if let Some(minion @ J::Object(_)) = doc.get("minion") {
            for (k, l) in flatten(&tree(minion)) {
                out.insert(format!("Minion.{k}"), leaf_json(&l));
            }
        }
    } else {
        for (k, l) in flatten(&tree(doc)) {
            out.insert(k, leaf_json(&l));
        }
    }
    out
}

/// Result of comparing one build's flattened outputs.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OutputsDiff {
    pub shared: usize,
    pub matching: usize,
    /// PoB keys absent from ours.
    pub missing: usize,
    /// Our keys absent from PoB.
    pub extra: usize,
    /// Mismatches dropped by a unit filter.
    pub filtered: usize,
    pub mismatches: Vec<Mismatch>,
}

/// Elements of the list at `prefix` in a flat map: index -> sorted
/// (suffix, value) pairs.
fn list_elements(flat: &BTreeMap<String, J>, prefix: &str) -> Vec<Vec<(String, J)>> {
    let mut els: BTreeMap<usize, Vec<(String, J)>> = BTreeMap::new();
    let pre = format!("{prefix}.");
    for (k, v) in flat.range(pre.clone()..) {
        let Some(rest) = k.strip_prefix(&pre) else {
            break;
        };
        let (idx, tail) = rest.split_once('.').unwrap_or((rest, ""));
        if let Ok(i) = idx.parse::<usize>() {
            els.entry(i).or_default().push((tail.to_owned(), v.clone()));
        }
    }
    els.into_values().collect()
}

/// Compare flattened outputs. `unit` keeps only mismatches whose PoB
/// writer lies in the given Lua ranges.
#[must_use]
pub fn diff_flat(
    pob: &BTreeMap<String, J>,
    ours: &BTreeMap<String, J>,
    opts: &DiffOptions<'_>,
    prov: Option<&Provenance>,
    unit: Option<&[LuaRange]>,
) -> OutputsDiff {
    let mut d = OutputsDiff::default();
    // lists whose order PoB does not fix compare as multisets of elements
    let mut unordered_done: BTreeSet<String> = BTreeSet::new();
    if let Some(orders) = opts.nondet_orders {
        for p in orders {
            let ea = list_elements(pob, p);
            let eb = list_elements(ours, p);
            let ja: Vec<J> = ea
                .iter()
                .map(|e| serde_json::to_value(e).unwrap_or(J::Null))
                .collect();
            let jb: Vec<J> = eb
                .iter()
                .map(|e| serde_json::to_value(e).unwrap_or(J::Null))
                .collect();
            let (l, r) = multiset_unmatched(&ja, &jb, opts.rel);
            if l.is_empty() && r.is_empty() {
                unordered_done.insert(format!("{p}."));
            }
        }
    }
    let in_unordered = |k: &str| unordered_done.iter().any(|p| k.starts_with(p.as_str()));
    for (k, pv) in pob {
        if in_unordered(k) {
            continue;
        }
        let Some(ov) = ours.get(k) else {
            d.missing += 1;
            continue;
        };
        d.shared += 1;
        if leaf_eq(pv, ov, opts.rel) || opts.accepted(k, ov) {
            d.matching += 1;
            continue;
        }
        let writer = prov.and_then(|p| p.writer(k)).map(str::to_owned);
        if let Some(ranges) = unit {
            if !writer
                .as_deref()
                .is_some_and(|w| writer_in_ranges(w, ranges))
            {
                d.filtered += 1;
                continue;
            }
        }
        d.mismatches.push(Mismatch {
            path: k.clone(),
            kind: MismatchKind::Value,
            pob: Some(pv.clone()),
            ours: Some(ov.clone()),
            writer,
        });
    }
    d.extra = ours.keys().filter(|k| !pob.contains_key(*k)).count();
    d
}

/// The `{build, pob, ours}` document report.py reads (plus the mismatch
/// list and counts).
#[must_use]
pub fn calc_diff_doc(
    build: &str,
    pob: &BTreeMap<String, J>,
    ours: &BTreeMap<String, J>,
    d: &OutputsDiff,
    unit: Option<&str>,
) -> J {
    let to_obj = |m: &BTreeMap<String, J>| {
        J::Object(m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
    };
    serde_json::json!({
        "build": build,
        "pob": to_obj(pob),
        "ours": to_obj(ours),
        "mismatches": d.mismatches,
        "summary": {
            "shared": d.shared,
            "matching": d.matching,
            "missing": d.missing,
            "extra": d.extra,
            "filtered": d.filtered,
            "unit": unit,
            "tolerance": REL_TOL,
        },
    })
}

// ---- unit ranges --------------------------------------------------------------------------------

/// A Lua line range of a unit (`file` as written in the unit table: a
/// basename like `CalcPerform.lua` or a path like `Data/Skills/act_int.lua`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LuaRange {
    pub file: String,
    pub first: usize,
    pub last: usize,
}

/// The "Lua sources" cell of `unit`'s row in calc-architecture.md §11.1,
/// parsed: `File.lua a-b, c-d; Other.lua e-f`; a file without ranges
/// covers the whole file.
pub fn unit_ranges(doc_md: &str, unit: &str) -> Result<Vec<LuaRange>> {
    let row_id = |l: &str| -> Option<String> {
        l.strip_prefix("| **")?
            .split("**")
            .next()
            .map(|id| id.trim().to_owned())
    };
    let exact = doc_md.lines().find(|l| row_id(l).as_deref() == Some(unit));
    // a grouped row `DF-B..G` holds one `/`-separated range list per unit
    let grouped = || {
        let (prefix, letter) = unit.rsplit_once('-')?;
        let mut lc = letter.chars();
        let (u, None) = (lc.next()?, lc.next()) else {
            return None;
        };
        doc_md.lines().find_map(|l| {
            let id = row_id(l)?;
            let (p, span) = id.rsplit_once('-')?;
            let (a, b) = span.split_once("..")?;
            let (a, b) = (a.chars().next()?, b.chars().next()?);
            (p == prefix && a <= u && u <= b).then_some((l, (u as u8 - a as u8) as usize))
        })
    };
    let (row, part) = match exact {
        Some(l) => (l, None),
        None => grouped()
            .map(|(l, i)| (l, Some(i)))
            .ok_or_else(|| OracleError(format!("unit {unit} not in the unit table")))?,
    };
    let cells: Vec<&str> = row.split('|').collect();
    let mut raw = cells.get(2).copied().unwrap_or("").replace('`', "");
    if let Some(i) = part {
        let parts: Vec<&str> = raw.split(" / ").collect();
        let file = parts
            .first()
            .and_then(|p| p.split_whitespace().find(|t| t.ends_with(".lua")))
            .unwrap_or("")
            .to_owned();
        let mine = parts
            .get(i)
            .ok_or_else(|| OracleError(format!("unit {unit}: no range #{} in {raw:?}", i + 1)))?;
        raw = if mine.contains(".lua") {
            (*mine).to_owned()
        } else {
            format!("{file} {mine}")
        };
    }
    // drop parenthesised notes: "closures (68)", "306 (sanitiseText)"
    let mut cell = String::with_capacity(raw.len());
    let mut depth = 0usize;
    for c in raw.chars() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            c if depth == 0 => cell.push(c),
            _ => {}
        }
    }
    let mut out = Vec::new();
    for seg in cell.split(';') {
        let mut file: Option<String> = None;
        let mut had_range = false;
        for tok in seg.split(|c: char| c.is_whitespace() || c == ',') {
            if tok.ends_with(".lua") {
                if let (Some(f), false) = (&file, had_range) {
                    out.push(LuaRange {
                        file: f.clone(),
                        first: 1,
                        last: usize::MAX,
                    });
                }
                file = Some(tok.to_owned());
                had_range = false;
                continue;
            }
            let Some(f) = &file else { continue };
            let (a, b) = tok.split_once('-').unwrap_or((tok, tok));
            if let (Ok(a), Ok(b)) = (a.parse::<usize>(), b.parse::<usize>()) {
                out.push(LuaRange {
                    file: f.clone(),
                    first: a,
                    last: b,
                });
                had_range = true;
            }
        }
        if let (Some(f), false) = (file, had_range) {
            out.push(LuaRange {
                file: f,
                first: 1,
                last: usize::MAX,
            });
        }
    }
    if out.is_empty() {
        return Err(OracleError(format!(
            "unit {unit}: no Lua ranges in {cell:?}"
        )));
    }
    Ok(out)
}

/// Whether a writer `path/File.lua:line` lies in one of the ranges.
#[must_use]
pub fn writer_in_ranges(writer: &str, ranges: &[LuaRange]) -> bool {
    let Some((file, line)) = writer.rsplit_once(':') else {
        return false;
    };
    let Ok(line) = line.parse::<usize>() else {
        return false;
    };
    ranges.iter().any(|r| {
        let same = file == r.file
            || file.ends_with(&format!("/{}", r.file))
            || r.file.ends_with(&format!("/{file}"));
        same && r.first <= line && line <= r.last
    })
}

#[cfg(test)]
mod tests {
    //! GOLD1 moved `tolerance`, `stores_multiset_and_order`, `nondet_values_and_orders`,
    //! `unordered_lists_with_nested_orders` and `store_alternatives_per_name` to
    //! `crates/pob-golden/src/compare.rs`, rewritten to the golden markers
    //! (docs/standalone/GOLDEN-PLAN.md §1.7). `units` stays: it tests the Lua unit ranges, which
    //! go with this crate.
    use super::*;

    #[test]
    fn units() {
        let md = "| **PF-A** perform helpers A | CalcPerform.lua 1-635 | x |\n\
                  | **CT** calc tools | CalcTools.lua 1-303; Common.lua 477-560, 897-962 | y |\n\
                  | **SK-A** hooks | `Data/Skills/act_int.lua` closures (68) | z |";
        let r = unit_ranges(md, "CT").unwrap();
        assert_eq!(r.len(), 3);
        assert!(writer_in_ranges("Modules/Common.lua:900", &r));
        assert!(!writer_in_ranges("Modules/Common.lua:700", &r));
        let r = unit_ranges(md, "SK-A").unwrap();
        assert!(writer_in_ranges("Data/Skills/act_int.lua:5", &r));
        assert_eq!(unit_ranges(md, "PF-A").unwrap()[0].last, 635);
        let md = "| **DF-B..G** | CalcDefence.lua 659-1210 / 1211-1673 / 1681-2152 | x |";
        let r = unit_ranges(md, "DF-C").unwrap();
        assert_eq!(
            r,
            vec![LuaRange {
                file: "CalcDefence.lua".into(),
                first: 1211,
                last: 1673
            }]
        );
        assert_eq!(unit_ranges(md, "DF-B").unwrap()[0].first, 659);
        assert!(unit_ranges(md, "DF-E").is_err());
        assert!(unit_ranges(md, "DF-H").is_err());
    }
}
