//! The shared encodings (GOLDEN-PLAN §1.1) and the merge of K recording runs (§3.1).
//!
//! The recorder (pob-oracle's `pob_golden_record`, from PoB's side) and the adapters (from the
//! Rust side) both encode through these functions, so a golden and the value it is compared with
//! are written by one schema.
//!
//! * **Numbers** ([`num`]): finite integral values with `|v| <= 2^53` are JSON integers, other
//!   finite values the shortest round-trip decimal, and NaN / ±inf the marker
//!   `{"$num":"nan"|"inf"|"-inf"}`. `-0.0` stays a float (`-0.0`), so a re-record is byte-stable;
//!   `num_eq` treats ±0 as equal anyway.
//! * **Flags** ([`mod_flag_names`], [`keyword_flag_names`]): sorted names of the single-bit
//!   entries of PoB's `ModFlag` / `KeywordFlag` tables, never bit masks, so a renumbering cannot
//!   break a golden. A bit with no name is written as `0x...` hex, so nothing is lost.
//! * **Mods** ([`mod_json`], `mod/1`): `{"flags","keywordFlags","name","source","tags","type",
//!   "value"}`; object keys are sorted (serde_json's map), tags are objects, lists of mods keep
//!   PoB's order.
//! * **Runs** ([`merge_runs`], [`merge_run_maps`]): one value per run becomes the value, a
//!   permuted list `{"$unordered":[…]}`, anything else `{"$anyOf":[…]}`; a key some runs lack
//!   gets `{"$absent":true}` as an alternative.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value as J};

/// 2^53: the integers an f64 holds exactly, all of them.
const EXACT_INT: f64 = 9_007_199_254_740_992.0;

/// A number in the golden encoding (see the module docs).
#[must_use]
pub fn num(v: f64) -> J {
    if v.is_nan() {
        return marker("$num", J::String("nan".into()));
    }
    if v.is_infinite() {
        let s = if v > 0.0 { "inf" } else { "-inf" };
        return marker("$num", J::String(s.into()));
    }
    if v.fract() == 0.0 && v.abs() <= EXACT_INT && !(v == 0.0 && v.is_sign_negative()) {
        return J::Number((v as i64).into());
    }
    serde_json::Number::from_f64(v).map_or(J::Null, J::Number)
}

/// The number a golden value holds: a JSON number or a `$num` marker.
#[must_use]
pub fn decode_num(v: &J) -> Option<f64> {
    match v {
        J::Number(n) => n.as_f64(),
        J::Object(m) if m.len() == 1 => match m.get("$num")?.as_str()? {
            "nan" => Some(f64::NAN),
            "inf" => Some(f64::INFINITY),
            "-inf" => Some(f64::NEG_INFINITY),
            _ => None,
        },
        _ => None,
    }
}

/// `{name: value}`.
#[must_use]
pub fn marker(name: &str, value: J) -> J {
    let mut m = Map::new();
    m.insert(name.to_owned(), value);
    J::Object(m)
}

/// Rewrite every number of `v` into its [`num`] form (a float that holds an integer becomes the
/// integer), so equal values always serialise to the same bytes.
pub fn canonicalize(v: &mut J) {
    match v {
        J::Number(n) => {
            if !(n.is_i64() || n.is_u64()) {
                if let Some(f) = n.as_f64() {
                    *v = num(f);
                }
            }
        }
        J::Array(a) => a.iter_mut().for_each(canonicalize),
        J::Object(m) => m.values_mut().for_each(canonicalize),
        _ => {}
    }
}

/// The canonical text of a value: compact JSON, object keys sorted (serde_json's default map),
/// numbers as [`canonicalize`] writes them.
#[must_use]
pub fn canonical(v: &J) -> String {
    let mut c = v.clone();
    canonicalize(&mut c);
    serde_json::to_string(&c).unwrap_or_default()
}

/// Sorted names of the single-bit `entries` set in `bits`; bits without a name as `0x…` hex.
#[must_use]
pub fn flag_names(bits: u32, entries: &[(&str, u32)]) -> Vec<String> {
    let mut out = BTreeSet::new();
    let mut named = 0u32;
    for &(name, v) in entries {
        if v.is_power_of_two() && bits & v != 0 {
            out.insert(name.to_owned());
            named |= v;
        }
    }
    let rest = bits & !named;
    for bit in 0..32 {
        let b = 1u32 << bit;
        if rest & b != 0 {
            out.insert(format!("0x{b:08x}"));
        }
    }
    out.into_iter().collect()
}

/// The bits of `names` ([`flag_names`] read back); `None` for a name that is neither an entry
/// nor `0x…` hex.
#[must_use]
pub fn flag_bits(names: &[String], entries: &[(&str, u32)]) -> Option<u32> {
    let mut bits = 0;
    for n in names {
        bits |= match entries.iter().find(|(e, _)| e == n) {
            Some(&(_, v)) => v,
            None => u32::from_str_radix(n.strip_prefix("0x")?, 16).ok()?,
        };
    }
    Some(bits)
}

/// `ModFlag` names of a mod's `flags`.
#[must_use]
pub fn mod_flag_names(bits: u32) -> Vec<String> {
    flag_names(bits, pob_gamedata::ModFlag::ENTRIES)
}

/// `KeywordFlag` names of a mod's `keywordFlags`.
#[must_use]
pub fn keyword_flag_names(bits: u32) -> Vec<String> {
    flag_names(bits, pob_gamedata::KeywordFlag::ENTRIES)
}

/// A mod in the `mod/1` encoding. `value` and `tags` are already encoded (numbers through
/// [`num`], tags as objects); `source` is `null` when the mod has none.
#[must_use]
pub fn mod_json(
    name: &str,
    mod_type: &str,
    value: J,
    flags: u32,
    keyword_flags: u32,
    source: Option<&str>,
    tags: Vec<J>,
) -> J {
    let names = |v: Vec<String>| J::Array(v.into_iter().map(J::String).collect());
    let mut m = Map::new();
    m.insert("name".into(), J::String(name.to_owned()));
    m.insert("type".into(), J::String(mod_type.to_owned()));
    m.insert("value".into(), value);
    m.insert("flags".into(), names(mod_flag_names(flags)));
    m.insert(
        "keywordFlags".into(),
        names(keyword_flag_names(keyword_flags)),
    );
    m.insert(
        "source".into(),
        source.map_or(J::Null, |s| J::String(s.to_owned())),
    );
    m.insert("tags".into(), J::Array(tags));
    J::Object(m)
}

/// The keys of a `mod/1` object.
pub const MOD_KEYS: [&str; 7] = [
    "flags",
    "keywordFlags",
    "name",
    "source",
    "tags",
    "type",
    "value",
];

/// Whether `v` is a `mod/1` object: `name` and `type` strings and a `value`, and no key outside
/// [`MOD_KEYS`].
#[must_use]
pub fn is_mod(v: &J) -> bool {
    let J::Object(m) = v else { return false };
    m.get("name").is_some_and(J::is_string)
        && m.get("type").is_some_and(J::is_string)
        && m.contains_key("value")
        && m.keys().all(|k| MOD_KEYS.contains(&k.as_str()))
}

/// Whether `v` is a non-empty list of `mod/1` objects (the compare engine matches such lists as
/// a multiset per mod name, §3.5).
#[must_use]
pub fn is_mod_list(v: &J) -> bool {
    matches!(v, J::Array(a) if !a.is_empty() && a.iter().all(is_mod))
}

/// The `{"$absent":true}` marker.
#[must_use]
pub fn absent() -> J {
    marker("$absent", J::Bool(true))
}

/// The bag of canonical texts of a list's elements (a permutation has the same bag).
fn bag(a: &[J]) -> BTreeMap<String, usize> {
    let mut m = BTreeMap::new();
    for x in a {
        *m.entry(canonical(x)).or_insert(0) += 1;
    }
    m
}

/// Merge one value as K recording runs produced it (`None`: the run produced nothing).
///
/// * Every run gave the same value: that value.
/// * Every run gave a list, and the lists are permutations of each other: `{"$unordered":
///   <run 1's list>}`.
/// * Otherwise `{"$anyOf":[…]}` with the distinct values in first-seen order; a run that
///   produced nothing contributes `{"$absent":true}`.
///
/// Values already carrying `$unordered` (declared up front) keep it. An empty input, or runs
/// that all produced nothing, give `None`.
#[must_use]
pub fn merge_runs(runs: &[Option<J>]) -> Option<J> {
    let present: Vec<&J> = runs.iter().flatten().collect();
    if present.is_empty() {
        return None;
    }
    let first = present[0];
    let all_present = present.len() == runs.len();
    let first_text = canonical(first);
    if all_present && present.iter().all(|v| canonical(v) == first_text) {
        return Some(first.clone());
    }
    if all_present {
        if let J::Array(a) = first {
            let b = bag(a);
            if present
                .iter()
                .all(|v| matches!(v, J::Array(x) if bag(x) == b))
            {
                return Some(marker("$unordered", first.clone()));
            }
        }
    }
    let mut seen = BTreeSet::new();
    let mut alts = Vec::new();
    for r in runs {
        let v = r.clone().unwrap_or_else(absent);
        if seen.insert(canonical(&v)) {
            alts.push(v);
        }
    }
    Some(marker("$anyOf", J::Array(alts)))
}

/// [`merge_runs`] per key over K runs' flat outputs (the union of their keys).
#[must_use]
pub fn merge_run_maps(runs: &[BTreeMap<String, J>]) -> BTreeMap<String, J> {
    let keys: BTreeSet<&String> = runs.iter().flat_map(BTreeMap::keys).collect();
    let mut out = BTreeMap::new();
    for k in keys {
        let vals: Vec<Option<J>> = runs.iter().map(|r| r.get(k).cloned()).collect();
        if let Some(v) = merge_runs(&vals) {
            out.insert(k.clone(), v);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn numbers() {
        assert_eq!(num(1.0), json!(1));
        assert_eq!(num(-3.0), json!(-3));
        assert_eq!(num(0.5), json!(0.5));
        assert_eq!(num(f64::NAN), json!({"$num": "nan"}));
        assert_eq!(num(f64::NEG_INFINITY), json!({"$num": "-inf"}));
        assert_eq!(serde_json::to_string(&num(-0.0)).unwrap(), "-0.0");
        assert_eq!(serde_json::to_string(&num(1e300)).unwrap(), "1e+300");
        assert_eq!(num(2f64.powi(60)), json!(1_152_921_504_606_846_976.0));
        for v in [0.1, 1.0 / 3.0, 45.758_095_238_095_24, -2.5e-12, 1e300, -0.0] {
            assert_eq!(decode_num(&num(v)).unwrap().to_bits(), v.to_bits());
        }
        assert!(decode_num(&num(f64::NAN)).unwrap().is_nan());
        assert_eq!(
            canonical(&json!({"b": 2.0, "a": [1.5, 3.0]})),
            r#"{"a":[1.5,3],"b":2}"#
        );
    }

    #[test]
    fn flags() {
        use pob_gamedata::{KeywordFlag, ModFlag};
        let bits = ModFlag::Attack | ModFlag::Hit | ModFlag::Weapon1H;
        assert_eq!(mod_flag_names(bits), ["Attack", "Hit", "Weapon1H"]);
        // a mask writes its bits, not its name
        assert_eq!(mod_flag_names(ModFlag::SourceMask), ["Area", "Projectile"]);
        assert_eq!(mod_flag_names(0x20), ["0x00000020"]);
        let names = keyword_flag_names(KeywordFlag::MatchAll | KeywordFlag::Fire);
        assert_eq!(names, ["Fire", "MatchAll"]);
        assert_eq!(
            flag_bits(&names, KeywordFlag::ENTRIES),
            Some(KeywordFlag::MatchAll | KeywordFlag::Fire)
        );
        assert_eq!(
            flag_bits(&mod_flag_names(0x20 | 1), ModFlag::ENTRIES),
            Some(0x21)
        );
        assert_eq!(flag_bits(&["Nope".into()], ModFlag::ENTRIES), None);
        let m = mod_json("Life", "BASE", num(10.0), 0, 0, None, vec![]);
        assert!(is_mod(&m));
        assert_eq!(
            canonical(&m),
            r#"{"flags":[],"keywordFlags":[],"name":"Life","source":null,"tags":[],"type":"BASE","value":10}"#
        );
        assert!(is_mod_list(&json!([m])));
        assert!(!is_mod_list(&json!([])));
        assert!(!is_mod(
            &json!({"name": "x", "type": "BASE", "value": 1, "other": 2})
        ));
    }

    #[test]
    fn runs() {
        assert_eq!(
            merge_runs(&[Some(json!(1)), Some(json!(1.0))]),
            Some(json!(1))
        );
        assert_eq!(
            merge_runs(&[Some(json!(45.76)), Some(json!(45.758)), Some(json!(45.76))]),
            Some(json!({"$anyOf": [45.76, 45.758]}))
        );
        assert_eq!(
            merge_runs(&[Some(json!(["a", "b"])), Some(json!(["b", "a"]))]),
            Some(json!({"$unordered": ["a", "b"]}))
        );
        assert_eq!(
            merge_runs(&[Some(json!(["a"])), Some(json!(["a", "a"]))]),
            Some(json!({"$anyOf": [["a"], ["a", "a"]]}))
        );
        assert_eq!(
            merge_runs(&[None, Some(json!(2))]),
            Some(json!({"$anyOf": [{"$absent": true}, 2]}))
        );
        assert_eq!(merge_runs(&[None, None]), None);
        let a: BTreeMap<String, J> = [("x".into(), json!(1)), ("y".into(), json!(2))].into();
        let b: BTreeMap<String, J> = [("x".into(), json!(1)), ("z".into(), json!(3))].into();
        let m = merge_run_maps(&[a, b]);
        assert_eq!(m["x"], json!(1));
        assert_eq!(m["y"], json!({"$anyOf": [2, {"$absent": true}]}));
        assert_eq!(m["z"], json!({"$anyOf": [{"$absent": true}, 3]}));
    }
}
