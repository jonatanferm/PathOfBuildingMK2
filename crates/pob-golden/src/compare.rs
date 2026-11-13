//! The compare engine (GOLDEN-PLAN §3.5, the markers of §3.2).
//!
//! It took over `pob_oracle::diff`'s rules (GOLD1, §1.7): `num_close`, the leaf and tree
//! equality, and the mod-store multiset with its order report. The oracle's nondeterminism
//! record (paths of free orders, accepted values per path) became markers in the golden itself.
//!
//! * **Numbers**: `num_eq`, `|a-b| <= 1e-9 * max(1, |a|)` with `a` = PoB. NaN equals NaN, ±0 are
//!   equal, an infinity equals only itself. A JSON number and a `{"$num":…}` marker are both
//!   numbers.
//! * **Strings, booleans, null**: exact.
//! * **Lists**: in order, element by element, the same length.
//! * **Objects**: the same keys (a key PoB has and Rust lacks, or the reverse, is a failure; a
//!   member whose expected value is `{"$absent":true}` must be missing), each value compared.
//! * **Mod lists** (a non-empty list of `mod/1` objects, [`crate::encode::is_mod_list`]): a
//!   multiset per mod name. A mod PoB has and Rust lacks is `mod-missing`, the reverse
//!   `mod-extra`. When the multisets agree and only the order differs, the line passes and the
//!   order difference is reported ([`LineResult::order_notes`]), as the oracle's `LUA-ORDER`.
//! * **Markers** (expected side only):
//!   * `{"$anyOf":[v…]}`: Rust equals one of them (an alternative may be `$absent` or `$error`);
//!   * `{"$unordered":[v…]}`: a multiset, each element compared with every rule here;
//!   * `{"$error":msg}`: Rust must fail (the message is not compared);
//!   * `{"$host":{"arm64":a,"x64":b}}`: Rust must equal `b`; equal to `a` is reported as such;
//!   * `{"$known":id,"pob":v}`: counted, not compared ([`Status::Known`]);
//!   * `{"$perOrder":[v…]}`: Rust's answer is the list of its results, one per order of the
//!     line's input, and result `i` must equal `v[i]`;
//!   * `{"$absent":true}`: PoB produced nothing, so Rust must produce nothing.
//!
//! A line's [`Status`] is `Failed` when any mismatch was found, else `Known` when a `$known`
//! value was skipped, else `AnyOf` / `Host` when one of those accepted it, else `Exact`.

use std::collections::BTreeMap;

use serde_json::Value as J;

use crate::encode::{decode_num, is_mod};

/// §9.2 numeric tolerance (relative, with an absolute floor of 1).
pub const REL_TOL: f64 = 1e-9;

/// Numbers agree: `|pob-ours| <= rel * max(1, |pob|)`; NaN only with NaN, an infinity only with
/// itself.
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

/// [`num_close`] at [`REL_TOL`]: the `num_eq` tolerance of every golden.
#[must_use]
pub fn num_eq(pob: f64, ours: f64) -> bool {
    num_close(pob, ours, REL_TOL)
}

/// Rust's answer for one key: a value, or the error the Rust side raised.
pub type Answer = std::result::Result<J, String>;

/// How a line was settled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Status {
    Exact,
    /// Accepted as one of PoB's recorded alternatives (`$anyOf`).
    AnyOf,
    /// Accepted as the x64 text of a host-dependent value (`$host`).
    Host,
    /// A `$known` value: counted, not compared.
    Known,
    Failed,
}

/// What differs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Values differ.
    Value,
    /// PoB has it, Rust does not.
    Missing,
    /// Rust has it, PoB does not (a Rust-only key, or a key of an `$absent` value).
    Extra,
    /// Lists of different lengths.
    Length,
    /// Rust raised where PoB returned a value.
    RustError,
    /// PoB raised (`$error`), Rust returned a value.
    NotError,
    /// Rust printed PoB's arm64 text of a `$host` value, not the x64 one.
    HostArm64,
    /// A mod of PoB's list with no equal mod in Rust's.
    ModMissing,
    /// A mod of Rust's list with no equal mod in PoB's.
    ModExtra,
    /// None of the `$anyOf` alternatives.
    NoAlternative,
    /// A `$unordered` / `$perOrder` list that does not match.
    Unmatched,
    /// An overlay line whose PoB value is not the base file's (the runner).
    StaleOverlay,
}

impl Kind {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Value => "value",
            Self::Missing => "missing",
            Self::Extra => "extra",
            Self::Length => "length",
            Self::RustError => "rust-error",
            Self::NotError => "not-error",
            Self::HostArm64 => "host-arm64",
            Self::ModMissing => "mod-missing",
            Self::ModExtra => "mod-extra",
            Self::NoAlternative => "no-alternative",
            Self::Unmatched => "unmatched",
            Self::StaleOverlay => "stale-overlay",
        }
    }
}

/// One difference inside a line's value. `path` is relative to the line's value (`""` is the
/// value itself, `.x`, `[2]`, `#1` for the result of order 1).
#[derive(Debug, Clone, PartialEq)]
pub struct Mismatch {
    pub path: String,
    pub kind: Kind,
    pub expected: Option<J>,
    pub actual: Option<J>,
}

/// The outcome of one line.
#[derive(Debug, Clone, PartialEq)]
pub struct LineResult {
    pub status: Status,
    pub mismatches: Vec<Mismatch>,
    /// Paths of mod lists whose mods matched in another order (reported, not failed).
    pub order_notes: Vec<String>,
    /// Every compared number was bit-identical (NaN with NaN); lines without numbers count.
    pub bit_exact: bool,
    /// The `$known` id, when the line is [`Status::Known`].
    pub known: Option<String>,
}

impl LineResult {
    #[must_use]
    pub fn failed(&self) -> bool {
        self.status == Status::Failed
    }
}

/// The markers of §3.2.
#[derive(Debug, Clone, Copy)]
enum Marker<'a> {
    AnyOf(&'a [J]),
    Unordered(&'a [J]),
    Error,
    Host { arm64: Option<&'a J>, x64: &'a J },
    Known(&'a str),
    PerOrder(&'a [J]),
    Absent,
}

fn marker(value: &J) -> Option<Marker<'_>> {
    let J::Object(obj) = value else { return None };
    let arr = |name: &str| obj.get(name).and_then(J::as_array).map(Vec::as_slice);
    match obj.len() {
        1 => {
            let (name, inner) = obj.iter().next()?;
            match name.as_str() {
                "$anyOf" => arr(name).map(Marker::AnyOf),
                "$unordered" => arr(name).map(Marker::Unordered),
                "$perOrder" => arr(name).map(Marker::PerOrder),
                "$error" => Some(Marker::Error),
                "$absent" => (inner == &J::Bool(true)).then_some(Marker::Absent),
                "$host" => {
                    let host = inner.as_object()?;
                    Some(Marker::Host {
                        arm64: host.get("arm64"),
                        x64: host.get("x64")?,
                    })
                }
                _ => None,
            }
        }
        2 if obj.contains_key("pob") => obj.get("$known")?.as_str().map(Marker::Known),
        _ => None,
    }
}

/// Whether `v` is one of the markers (a `$num` is a number, not a marker).
#[must_use]
pub fn is_marker(v: &J) -> bool {
    marker(v).is_some()
}

/// The comparison state of one line.
struct Walk {
    rel: f64,
    mismatches: Vec<Mismatch>,
    order_notes: Vec<String>,
    any_of: bool,
    host: bool,
    known: Option<String>,
    bit_exact: bool,
}

impl Walk {
    fn new(rel: f64) -> Self {
        Self {
            rel,
            mismatches: Vec::new(),
            order_notes: Vec::new(),
            any_of: false,
            host: false,
            known: None,
            bit_exact: true,
        }
    }

    fn push(&mut self, path: &str, kind: Kind, expected: Option<&J>, actual: Option<&J>) {
        self.mismatches.push(Mismatch {
            path: path.to_owned(),
            kind,
            expected: expected.cloned(),
            actual: actual.cloned(),
        });
    }

    /// A trial comparison: `Some(walk)` when `exp` and `act` agree (the caller merges it).
    fn trial(&self, path: &str, exp: &J, act: &J) -> Option<Self> {
        let mut w = Self::new(self.rel);
        w.eq(path, exp, act);
        w.mismatches.is_empty().then_some(w)
    }

    /// Take a successful trial's flags and notes.
    fn merge(&mut self, w: Self) {
        self.order_notes.extend(w.order_notes);
        self.any_of |= w.any_of;
        self.host |= w.host;
        self.bit_exact &= w.bit_exact;
        if self.known.is_none() {
            self.known = w.known;
        }
    }

    /// Greedy multiset matching (each expected element takes the first unused actual element
    /// it equals): the indices of the unmatched elements of each side.
    fn match_bag(&mut self, path: &str, exp: &[J], act: &[J]) -> (Vec<usize>, Vec<usize>) {
        let mut used = vec![false; act.len()];
        let mut left = Vec::new();
        for (i, x) in exp.iter().enumerate() {
            let hit = (0..act.len()).find_map(|j| {
                if used[j] {
                    return None;
                }
                self.trial(path, x, &act[j]).map(|w| (j, w))
            });
            match hit {
                Some((j, w)) => {
                    used[j] = true;
                    self.merge(w);
                }
                None => left.push(i),
            }
        }
        let right = (0..act.len()).filter(|&j| !used[j]).collect();
        (left, right)
    }

    fn eq(&mut self, path: &str, exp: &J, act: &J) {
        if let Some(m) = marker(exp) {
            return self.eq_marker(path, m, exp, act);
        }
        if let Some(x) = decode_num(exp) {
            match decode_num(act) {
                Some(y) if num_close(x, y, self.rel) => {
                    if !(x.to_bits() == y.to_bits() || (x.is_nan() && y.is_nan())) {
                        self.bit_exact = false;
                    }
                }
                _ => self.push(path, Kind::Value, Some(exp), Some(act)),
            }
            return;
        }
        match (exp, act) {
            (J::Array(x), J::Array(y))
                if crate::encode::is_mod_list(exp) && y.iter().all(is_mod) =>
            {
                self.eq_mods(path, x, y);
            }
            (J::Array(x), J::Array(y)) => {
                if x.len() != y.len() {
                    return self.push(path, Kind::Length, Some(exp), Some(act));
                }
                for (i, (p, q)) in x.iter().zip(y).enumerate() {
                    self.eq(&format!("{path}[{i}]"), p, q);
                }
            }
            (J::Object(x), J::Object(y)) => {
                for (k, p) in x {
                    let sub = format!("{path}.{k}");
                    match (y.get(k), marker(p)) {
                        (None, Some(Marker::Absent)) => {}
                        (None, Some(Marker::Known(id))) => self.known_value(id),
                        (None, Some(Marker::AnyOf(alts)))
                            if alts
                                .iter()
                                .any(|a| matches!(marker(a), Some(Marker::Absent))) =>
                        {
                            self.any_of = true;
                        }
                        (Some(q), Some(Marker::Absent)) => {
                            self.push(&sub, Kind::Extra, None, Some(q));
                        }
                        (Some(q), _) => self.eq(&sub, p, q),
                        (None, _) => self.push(&sub, Kind::Missing, Some(p), None),
                    }
                }
                for (k, q) in y {
                    if !x.contains_key(k) {
                        self.push(&format!("{path}.{k}"), Kind::Extra, None, Some(q));
                    }
                }
            }
            _ => {
                if exp != act {
                    self.push(path, Kind::Value, Some(exp), Some(act));
                }
            }
        }
    }

    fn known_value(&mut self, id: &str) {
        if self.known.is_none() {
            self.known = Some(id.to_owned());
        }
    }

    fn eq_marker(&mut self, path: &str, m: Marker<'_>, exp: &J, act: &J) {
        match m {
            Marker::AnyOf(alts) => {
                let hit = alts.iter().find_map(|a| self.trial(path, a, act));
                match hit {
                    Some(w) => {
                        self.merge(w);
                        self.any_of = true;
                    }
                    None => self.push(path, Kind::NoAlternative, Some(exp), Some(act)),
                }
            }
            Marker::Unordered(x) => {
                let Some(y) = act.as_array() else {
                    return self.push(path, Kind::Value, Some(exp), Some(act));
                };
                let (left, right) = self.match_bag(path, x, y);
                if !left.is_empty() || !right.is_empty() {
                    self.push(path, Kind::Unmatched, Some(exp), Some(act));
                }
            }
            Marker::PerOrder(x) => {
                let Some(y) = act.as_array() else {
                    return self.push(path, Kind::Value, Some(exp), Some(act));
                };
                if x.len() != y.len() {
                    return self.push(path, Kind::Unmatched, Some(exp), Some(act));
                }
                for (i, (p, q)) in x.iter().zip(y).enumerate() {
                    self.eq(&format!("{path}#{i}"), p, q);
                }
            }
            Marker::Host { arm64, x64 } => {
                if let Some(w) = self.trial(path, x64, act) {
                    self.merge(w);
                    self.host = true;
                } else {
                    let kind = if arm64.is_some_and(|a| self.trial(path, a, act).is_some()) {
                        Kind::HostArm64
                    } else {
                        Kind::Value
                    };
                    self.push(path, kind, Some(x64), Some(act));
                }
            }
            Marker::Known(id) => self.known_value(id),
            // inside a value, a present Rust value where PoB produced nothing or raised
            Marker::Absent => self.push(path, Kind::Extra, None, Some(act)),
            Marker::Error => self.push(path, Kind::NotError, Some(exp), Some(act)),
        }
    }

    /// A mod list: a multiset per mod name, then the order check.
    fn eq_mods(&mut self, path: &str, exp: &[J], act: &[J]) {
        let by_name = |l: &[J]| {
            let mut m: BTreeMap<String, Vec<usize>> = BTreeMap::new();
            for (i, x) in l.iter().enumerate() {
                let n = x.get("name").and_then(J::as_str).unwrap_or_default();
                m.entry(n.to_owned()).or_default().push(i);
            }
            m
        };
        let (ea, eb) = (by_name(exp), by_name(act));
        let names: std::collections::BTreeSet<&String> = ea.keys().chain(eb.keys()).collect();
        let mut matched = true;
        for name in names {
            let pick = |m: &BTreeMap<String, Vec<usize>>, l: &[J]| -> Vec<J> {
                m.get(name)
                    .map(|ix| ix.iter().map(|&i| l[i].clone()).collect())
                    .unwrap_or_default()
            };
            let (la, lb) = (pick(&ea, exp), pick(&eb, act));
            let (left, right) = self.match_bag(path, &la, &lb);
            matched &= left.is_empty() && right.is_empty();
            for i in left {
                self.push(path, Kind::ModMissing, Some(&la[i]), None);
            }
            for j in right {
                self.push(path, Kind::ModExtra, None, Some(&lb[j]));
            }
        }
        if matched {
            let same_order = exp.len() == act.len()
                && exp
                    .iter()
                    .zip(act)
                    .all(|(x, y)| self.trial(path, x, y).is_some());
            if !same_order {
                self.order_notes.push(path.to_owned());
            }
        }
    }

    fn finish(self) -> LineResult {
        let status = if !self.mismatches.is_empty() {
            Status::Failed
        } else if self.known.is_some() {
            Status::Known
        } else if self.any_of {
            Status::AnyOf
        } else if self.host {
            Status::Host
        } else {
            Status::Exact
        };
        LineResult {
            status,
            mismatches: self.mismatches,
            order_notes: self.order_notes,
            bit_exact: self.bit_exact,
            known: self.known,
        }
    }
}

/// Compare one line: PoB's recorded `expected` against Rust's answer (`None`: Rust produced
/// nothing for the key).
#[must_use]
pub fn compare_line(expected: &J, actual: Option<&Answer>) -> LineResult {
    compare_line_rel(expected, actual, REL_TOL)
}

/// [`compare_line`] with another relative tolerance (tests).
#[must_use]
pub fn compare_line_rel(expected: &J, actual: Option<&Answer>, rel: f64) -> LineResult {
    let mut w = Walk::new(rel);
    match (marker(expected), actual) {
        (Some(Marker::Known(id)), _) => w.known_value(id),
        (Some(Marker::Absent), None) | (Some(Marker::Error), Some(Err(_))) => {}
        (Some(Marker::Absent), Some(a)) => {
            let shown = a
                .clone()
                .unwrap_or_else(|e| crate::encode::marker("$error", J::String(e)));
            w.push("", Kind::Extra, None, Some(&shown));
        }
        (Some(Marker::Error), Some(Ok(v))) => w.push("", Kind::NotError, Some(expected), Some(v)),
        (Some(Marker::AnyOf(alts)), _) => {
            // an alternative may be `$absent` or `$error`: try each as a whole line
            let hit = alts
                .iter()
                .map(|a| compare_line_rel(a, actual, rel))
                .find(|r| !r.failed());
            if let Some(mut r) = hit {
                if r.status == Status::Exact || r.status == Status::Host {
                    r.status = Status::AnyOf;
                }
                return r;
            }
            let shown = actual.map(|a| {
                a.clone()
                    .unwrap_or_else(|e| crate::encode::marker("$error", J::String(e)))
            });
            w.push("", Kind::NoAlternative, Some(expected), shown.as_ref());
        }
        (_, None) => w.push("", Kind::Missing, Some(expected), None),
        (_, Some(Err(e))) => w.push(
            "",
            Kind::RustError,
            Some(expected),
            Some(&J::String(e.clone())),
        ),
        (_, Some(Ok(v))) => w.eq("", expected, v),
    }
    w.finish()
}

/// What the negative control (§4.4) changes in a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutateKind {
    /// The first compared, finite, non-zero number, × (1 + 1e-6).
    Number,
    /// The first compared string, with `"\u{1}"` appended.
    String,
    /// The first compared boolean, negated.
    Bool,
}

/// The negative control's change of `v` (§4.4): `kind`'s first compared leaf in walk order
/// (array order, object keys sorted), skipping the subtrees that are not compared one to one
/// (`$known`, `$anyOf`, `$unordered`, `$host`, `$perOrder`, `$absent`, `$error`). `None` when
/// `v` has no such leaf.
#[must_use]
pub fn mutate(v: &J, kind: MutateKind) -> Option<J> {
    fn walk(v: &mut J, kind: MutateKind) -> bool {
        if marker(v).is_some() {
            return false;
        }
        if let Some(x) = decode_num(v) {
            if kind == MutateKind::Number && x.is_finite() && x != 0.0 {
                *v = serde_json::Number::from_f64(x * (1.0 + 1e-6)).map_or(J::Null, J::Number);
                return true;
            }
            return false;
        }
        match v {
            J::String(s) if kind == MutateKind::String => {
                s.push('\u{1}');
                true
            }
            J::Bool(b) if kind == MutateKind::Bool => {
                *b = !*b;
                true
            }
            J::Array(a) => a.iter_mut().any(|x| walk(x, kind)),
            J::Object(m) => m.values_mut().any(|x| walk(x, kind)),
            _ => false,
        }
    }
    let mut out = v.clone();
    walk(&mut out, kind).then_some(out)
}

#[cfg(test)]
mod tests {
    //! The oracle's diff tests (`pob_oracle::diff` `tolerance`, `stores_multiset_and_order`,
    //! `nondet_values_and_orders`, `unordered_lists_with_nested_orders`,
    //! `store_alternatives_per_name`), moved by GOLD1 (GOLDEN-PLAN §1.7) with their inputs
    //! rewritten from the oracle's `$store` and nondet-path forms to the golden markers.
    #![allow(clippy::many_single_char_names)]
    use super::*;
    use serde_json::json;

    fn ok(v: J) -> Answer {
        Ok(v)
    }

    fn cmp(exp: &J, act: &J) -> LineResult {
        compare_line(exp, Some(&ok(act.clone())))
    }

    fn m(n: &str, v: i64) -> J {
        crate::encode::mod_json(n, "BASE", json!(v), 0, 0, None, vec![])
    }

    #[test]
    fn tolerance() {
        assert!(num_close(1e12, 1e12 + 1.0, REL_TOL));
        assert!(!num_close(1.0, 1.0 + 1e-8, REL_TOL));
        assert!(num_close(0.0, 5e-10, REL_TOL));
        assert!(num_close(f64::NAN, f64::NAN, REL_TOL));
        assert!(!num_close(f64::NAN, 1.0, REL_TOL) && !num_close(1.0, f64::NAN, REL_TOL));
        assert!(!num_close(f64::INFINITY, f64::NEG_INFINITY, REL_TOL));
        assert!(num_eq(f64::INFINITY, f64::INFINITY));
        // ±0 is a host artefact (CCR-0001/0002)
        assert!(num_close(-0.0, 0.0, REL_TOL) && num_close(0.0, -0.0, REL_TOL));
        let r = cmp(&json!(-0.0), &json!(0));
        assert_eq!(r.status, Status::Exact);
        assert!(!r.bit_exact, "±0 differ in their bits");
        assert_eq!(
            cmp(&json!({"$num": "inf"}), &json!({"$num": "inf"})).status,
            Status::Exact
        );
        assert!(cmp(&json!({"$num": "inf"}), &json!(1e308)).failed());
        assert_eq!(
            cmp(&json!({"$num": "nan"}), &json!({"$num": "nan"})).status,
            Status::Exact
        );
        // the relative tolerance is against PoB's value, with a floor of 1
        assert!(cmp(&json!(100.0), &json!(100.000_001)).failed());
        assert!(!cmp(&json!(1e6), &json!(1e6 + 1e-4)).failed());
        let r = cmp(&json!(3), &json!(3.000_000_000_1));
        assert_eq!(r.status, Status::Exact);
        assert!(!r.bit_exact);
        assert!(cmp(&json!(3), &json!(3)).bit_exact);
        // strings and booleans exact, types matter
        assert!(cmp(&json!("a"), &json!("a ")).failed());
        assert!(cmp(&json!(true), &json!(1)).failed());
        assert!(cmp(&json!(1), &json!("1")).failed());
    }

    /// Was: two `$store`s whose mods per name agree as a multiset, in another order.
    #[test]
    fn stores_multiset_and_order() {
        let mut a1 = m("Life", 1);
        a1["source"] = json!("Tree:1");
        let pob = json!([a1, m("Life", 2)]);
        let rust = json!([m("Life", 2), a1]);
        let r = cmp(&pob, &rust);
        assert_eq!(
            r.status,
            Status::Exact,
            "an order difference is reported, not failed"
        );
        assert_eq!(r.order_notes, vec![String::new()]);
        assert!(cmp(&pob, &pob).order_notes.is_empty());
        // a mod PoB has and Rust lacks
        let r = cmp(&pob, &json!([m("Life", 2)]));
        assert!(r.failed());
        assert_eq!(r.mismatches.len(), 1);
        assert_eq!(r.mismatches[0].kind, Kind::ModMissing);
        assert_eq!(r.mismatches[0].expected, Some(a1.clone()));
        // and one Rust has extra (a mod of another name too)
        let r = cmp(&pob, &json!([a1, m("Life", 2), m("Mana", 3)]));
        assert_eq!(
            r.mismatches.iter().map(|x| x.kind).collect::<Vec<_>>(),
            [Kind::ModExtra]
        );
        // a mod list nested in an object is still a mod list
        let r = cmp(&json!({"mods": pob}), &json!({"mods": rust}));
        assert_eq!(r.status, Status::Exact);
        assert_eq!(r.order_notes, vec![".mods".to_owned()]);
        // the mods' values compare with the tolerance
        let near = json!([
            a1,
            crate::encode::mod_json("Life", "BASE", json!(2.000_000_000_1), 0, 0, None, vec![])
        ]);
        assert!(!cmp(&pob, &near).failed());
    }

    /// Was: a nondet record with `y`'s values and `x`'s free order.
    #[test]
    fn nondet_values_and_orders() {
        let exp = json!({"x": {"$unordered": [1, 2]}, "y": {"$anyOf": [1, 3]}});
        let act = json!({"x": [2, 1], "y": 3});
        let r = cmp(&exp, &act);
        assert_eq!(r.status, Status::AnyOf);
        assert!(r.mismatches.is_empty());
        // without the markers: x[0], x[1] and y differ
        let strict = json!({"x": [1, 2], "y": 1});
        let r = cmp(&strict, &act);
        assert_eq!(r.mismatches.len(), 3, "{:?}", r.mismatches);
        assert_eq!(
            r.mismatches
                .iter()
                .map(|x| x.path.as_str())
                .collect::<Vec<_>>(),
            [".x[0]", ".x[1]", ".y"]
        );
        // a value outside the alternatives fails, and so does a list that is not a permutation
        assert!(cmp(&exp, &json!({"x": [2, 1], "y": 2})).failed());
        let r = cmp(&exp, &json!({"x": [2, 2], "y": 1}));
        assert_eq!(r.mismatches[0].kind, Kind::Unmatched);
        // an $unordered list of another length
        assert!(cmp(&exp, &json!({"x": [2, 1, 1], "y": 1})).failed());
    }

    /// Was: an unordered `buffs` list whose elements carry a nested reordered list.
    #[test]
    fn unordered_lists_with_nested_orders() {
        // the list and the nested list it reordered are both order-free
        let exp = json!({"buffs": {"$unordered": [
            {"n": "A", "mods": {"$unordered": [1, 2]}},
            {"n": "B", "mods": [3]}]}});
        let b = json!({"buffs": [{"n": "B", "mods": [3]}, {"n": "A", "mods": [2, 1]}]});
        assert_eq!(cmp(&exp, &b).status, Status::Exact);
        let c = json!({"buffs": [{"n": "B", "mods": [4]}, {"n": "A", "mods": [2, 1]}]});
        let r = cmp(&exp, &c);
        assert_eq!(r.mismatches.len(), 1);
        assert_eq!(r.mismatches[0].path, ".buffs");
        // numbers inside still compare with the tolerance
        let d =
            json!({"buffs": [{"n": "B", "mods": [3.000_000_000_001]}, {"n": "A", "mods": [2, 1]}]});
        assert!(!cmp(&exp, &d).failed());
        // without a nested marker below it, only the list's own order is free
        let only = json!({"buffs": {"$unordered": [
            {"n": "A", "mods": [1, 2]},
            {"n": "B", "mods": [3]}]}});
        assert!(cmp(&only, &b).failed());
        let e = json!({"buffs": [{"n": "B", "mods": [3]}, {"n": "A", "mods": [1, 2]}]});
        assert!(!cmp(&only, &e).failed());
    }

    /// Was: a store whose mods per name differ between PoB runs (nondet alternatives).
    #[test]
    fn store_alternatives_per_name() {
        // flattened per name: one line per mod name; Mana was absent in one run
        let life = json!({"$anyOf": [[m("Life", 1), m("Life", 2)], [m("Life", 3)]]});
        let mana = json!({"$anyOf": [[m("Mana", 5)], {"$absent": true}]});
        // another run: Life holds other mods, Mana is absent
        assert_eq!(cmp(&life, &json!([m("Life", 3)])).status, Status::AnyOf);
        assert_eq!(compare_line(&mana, None).status, Status::AnyOf);
        // without the alternatives, both lines fail (two mods missing, one extra; a missing key)
        let strict_life = json!([m("Life", 1), m("Life", 2)]);
        let r = cmp(&strict_life, &json!([m("Life", 3)]));
        assert_eq!(r.mismatches.len(), 3, "{:?}", r.mismatches);
        assert!(compare_line(&json!([m("Mana", 5)]), None).failed());
        // an alternative compares as a multiset; anything else still differs
        let r = cmp(&life, &json!([m("Life", 2), m("Life", 1)]));
        assert_eq!(r.status, Status::AnyOf);
        assert_eq!(r.order_notes.len(), 1);
        assert_eq!(cmp(&mana, &json!([m("Mana", 5)])).status, Status::AnyOf);
        let r = cmp(&life, &json!([m("Life", 4)]));
        assert!(r.failed());
        assert_eq!(r.mismatches[0].kind, Kind::NoAlternative);
    }

    #[test]
    fn markers() {
        // $error: Rust must fail; the message is not compared
        let e = json!({"$error": "bad"});
        assert_eq!(
            compare_line(&e, Some(&Err("other".into()))).status,
            Status::Exact
        );
        assert_eq!(cmp(&e, &json!(1)).mismatches[0].kind, Kind::NotError);
        assert_eq!(compare_line(&e, None).mismatches[0].kind, Kind::Missing);
        // and a Rust error where PoB returned a value
        let r = compare_line(&json!(1), Some(&Err("boom".into())));
        assert_eq!(r.mismatches[0].kind, Kind::RustError);
        // $anyOf with an $error alternative
        let a = json!({"$anyOf": [1, {"$error": "x"}]});
        assert_eq!(
            compare_line(&a, Some(&Err("y".into()))).status,
            Status::AnyOf
        );
        // $host: Rust must print the x64 text
        let h = json!({"$host": {"arm64": "-2147483648", "x64": "-9223372036854775808"}});
        assert_eq!(cmp(&h, &json!("-9223372036854775808")).status, Status::Host);
        assert_eq!(
            cmp(&h, &json!("-2147483648")).mismatches[0].kind,
            Kind::HostArm64
        );
        assert_eq!(cmp(&h, &json!("0")).mismatches[0].kind, Kind::Value);
        // $known: counted, not compared, wherever it is
        let k = json!({"$known": "CCR-0014", "pob": 7});
        let r = cmp(&k, &json!(8));
        assert_eq!(
            (r.status, r.known.as_deref()),
            (Status::Known, Some("CCR-0014"))
        );
        assert_eq!(compare_line(&k, None).status, Status::Known);
        let r = cmp(&json!({"a": 1, "b": k}), &json!({"a": 1}));
        assert_eq!(r.status, Status::Known);
        assert!(cmp(&json!({"a": 1, "b": k}), &json!({"a": 2, "b": 0})).failed());
        // $absent: Rust must have nothing (as a line or as a member)
        let ab = json!({"$absent": true});
        assert_eq!(compare_line(&ab, None).status, Status::Exact);
        assert_eq!(cmp(&ab, &json!(0)).mismatches[0].kind, Kind::Extra);
        assert_eq!(
            cmp(&json!({"a": 1, "b": ab}), &json!({"a": 1})).status,
            Status::Exact
        );
        let maybe = json!({"a": 1, "b": {"$anyOf": [2, ab]}});
        assert_eq!(cmp(&maybe, &json!({"a": 1})).status, Status::AnyOf);
        assert_eq!(cmp(&maybe, &json!({"a": 1, "b": 2})).status, Status::AnyOf);
        assert!(cmp(&maybe, &json!({"a": 1, "b": 3})).failed());
        assert!(cmp(&json!({"a": 1, "b": ab}), &json!({"a": 1, "b": 2})).failed());
        // $perOrder: one Rust result per stored order
        let p = json!({"$perOrder": [10, {"$num": "nan"}]});
        assert_eq!(cmp(&p, &json!([10, {"$num": "nan"}])).status, Status::Exact);
        let r = cmp(&p, &json!([10, 3]));
        assert_eq!(r.mismatches[0].path, "#1");
        assert_eq!(cmp(&p, &json!([10])).mismatches[0].kind, Kind::Unmatched);
        // object keys: missing and extra members
        let r = cmp(&json!({"a": 1, "b": 2}), &json!({"a": 1, "c": 2}));
        assert_eq!(
            r.mismatches
                .iter()
                .map(|x| (x.path.as_str(), x.kind))
                .collect::<Vec<_>>(),
            [(".b", Kind::Missing), (".c", Kind::Extra)]
        );
        assert_eq!(
            cmp(&json!([1, 2]), &json!([1])).mismatches[0].kind,
            Kind::Length
        );
        // object references compare as plain objects
        assert!(!cmp(&json!({"$item": "Ring 1"}), &json!({"$item": "Ring 1"})).failed());
        assert!(cmp(&json!({"$item": "Ring 1"}), &json!({"$item": "Ring 2"})).failed());
        assert!(compare_line(&json!(1), None).failed());
    }

    #[test]
    fn negative_control_mutation() {
        let v =
            json!({"a": 0, "b": {"$known": "X", "pob": 5}, "c": [{"$anyOf": [1, 2]}, 7], "d": "s"});
        let n = mutate(&v, MutateKind::Number).unwrap();
        assert_eq!(n["c"][1], json!(7.0 * (1.0 + 1e-6)));
        assert!(cmp(&v, &n).failed());
        assert!(cmp(&n, &v).failed());
        let s = mutate(&v, MutateKind::String).unwrap();
        assert_eq!(s["d"], json!("s\u{1}"));
        assert!(mutate(&v, MutateKind::Bool).is_none());
        assert_eq!(
            mutate(&json!([false]), MutateKind::Bool),
            Some(json!([true]))
        );
        // markers and zeros are not candidates
        assert!(mutate(&json!({"$unordered": [1]}), MutateKind::Number).is_none());
        assert!(mutate(&json!([0, {"$num": "inf"}]), MutateKind::Number).is_none());
    }
}
