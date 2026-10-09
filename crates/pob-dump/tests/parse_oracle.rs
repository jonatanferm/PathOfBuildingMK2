//! `parse_oracle.jsonl`: size, shape, provenance of every reported pattern
//! line, agreement with PoB's own ModCache, determinism, and that the scan
//! prefilter never changes a result.
//!
//! Needs PoB's source: `$POB_SRC`, or `<workspace>/.PathOfBuilding/src`, or
//! the main checkout's (worktrees). Skipped (with a message) when absent.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

fn pob_src() -> Option<PathBuf> {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let candidates = [
        std::env::var_os("POB_SRC").map(PathBuf::from),
        Some(here.join("../../.PathOfBuilding/src")),
        // a git worktree under <main checkout>/.claude/worktrees/<name>
        Some(here.join("../../../../../.PathOfBuilding/src")),
    ];
    candidates
        .into_iter()
        .flatten()
        .find(|p| p.join("Modules/Data.lua").exists())
}

/// Numbers equal to 13 significant digits (ModCache.lua is written with
/// fewer digits than a double has). Zeros must agree in sign: ModCache.lua
/// was written by x86-64 PoB, whose `-tonumber("0")` is -0.
fn num_of(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::Object(o) if o.len() == 1 && o.get("$float") == Some(&Value::from("-0")) => {
            Some(-0.0)
        }
        _ => None,
    }
}

fn same(a: &Value, b: &Value) -> bool {
    if let (Some(x), Some(y)) = (num_of(a), num_of(b)) {
        if x == 0.0 || y == 0.0 {
            return x == y && x.is_sign_negative() == y.is_sign_negative();
        }
        return x == y || (x - y).abs() <= 1e-13 * x.abs().max(y.abs());
    }
    match (a, b) {
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| same(p, q))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| same(v, w)))
        }
        _ => a == b,
    }
}

/// Lua source form of a key as ModParser.lua writes it in a table entry
/// (`["key"`; some entries have a space before the `]`, e.g. ModParser.lua:4783).
fn lua_key(k: &str) -> String {
    format!("[\"{}\"", k.replace('\\', "\\\\").replace('"', "\\\""))
}

#[test]
fn full_corpus() {
    let Some(src) = pob_src() else {
        eprintln!("skipping: PoB source not found (set POB_SRC)");
        return;
    };
    let out = Path::new(env!("CARGO_TARGET_TMPDIR")).join("pob-dump-parse-oracle");
    let report = pob_dump::dump(&pob_dump::Options {
        pob_src: src.clone(),
        out: out.clone(),
        pretty: true,
        skip_trees: true,
        parse_oracle: true,
    })
    .unwrap();
    eprintln!("timings {:?}", report.timings);
    let manifest: Value =
        serde_json::from_str(&std::fs::read_to_string(out.join("manifest.json")).unwrap()).unwrap();
    let info = &manifest["parseOracle"];
    eprintln!("summary: {}", serde_json::to_string_pretty(info).unwrap());
    let records = info["records"].as_u64().unwrap();
    assert!(records >= 80_000, "only {records} distinct lines");
    assert_eq!(info["errors"], 0, "parseMod raised Lua errors");
    assert_eq!(
        info["unresolvedSteps"], 0,
        "scan winners without a source line"
    );
    let empty = |v: &Value| {
        v.as_array().map_or(true, Vec::is_empty) && v.as_object().map_or(true, |o| o.is_empty())
    };
    assert!(
        empty(&info["unresolvedKeys"]),
        "pattern keys without a source line: {}",
        info["unresolvedKeys"]
    );
    assert!(
        empty(&info["runErrors"]),
        "corpus parts failed: {}",
        info["runErrors"]
    );
    for (part, min) in [
        ("modCache", 23_000),
        ("tree:2_6", 100),
        ("affix:Explicit", 1000),
    ] {
        assert!(
            info["counts"][part].as_u64().unwrap() >= min,
            "{part}: {}",
            info["counts"][part]
        );
    }
    let entry = manifest["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["file"] == "parse_oracle.jsonl")
        .expect("parse_oracle.jsonl in manifest");

    let text = std::fs::read_to_string(out.join("parse_oracle.jsonl")).unwrap();
    assert_eq!(entry["bytes"].as_u64().unwrap(), text.len() as u64);
    let modparser: Vec<String> = std::fs::read_to_string(src.join("Modules/ModParser.lua"))
        .unwrap()
        .split('\n')
        .map(str::to_owned)
        .collect();
    let mut by_line: BTreeMap<String, Value> = BTreeMap::new();
    let mut prev: Option<String> = None;
    let (mut steps, mut matched, mut with_fn) = (0usize, 0usize, 0usize);
    let mut lists: BTreeMap<String, usize> = BTreeMap::new();
    let mut labels = std::collections::BTreeSet::new();
    for l in text.lines() {
        let r: Value = serde_json::from_str(l).unwrap();
        let keys: Vec<&str> = r.as_object().unwrap().keys().map(String::as_str).collect();
        for k in ["line", "mods", "extra", "passes", "sources"] {
            assert!(keys.contains(&k), "record without {k}: {l}");
        }
        let line = r["line"].as_str().expect("line is a string").to_owned();
        if let Some(p) = &prev {
            assert!(
                p.as_bytes() < line.as_bytes(),
                "not sorted/unique at {line:?}"
            );
        }
        prev = Some(line.clone());
        assert!(!r["sources"].as_array().unwrap().is_empty());
        for s in r["sources"].as_array().unwrap() {
            let s = s.as_str().unwrap();
            labels.insert(s.split(':').next().unwrap().to_owned());
        }
        if l.contains("\"$fn\":") {
            with_fn += 1;
            assert!(l.contains("\"src\":\"Modules/ModParser.lua:"), "{l}");
        }
        let passes = r["passes"].as_array().unwrap();
        assert!(!passes.is_empty() && passes.len() <= 2, "{l}");
        for (i, p) in passes.iter().enumerate() {
            assert_eq!(p["order"], i as u64 + 1);
            for s in p["steps"].as_array().unwrap() {
                steps += 1;
                let list = s["list"].as_str().unwrap();
                assert_ne!(list, "?", "scan over an unknown table: {l}");
                *lists.entry(list.to_owned()).or_default() += 1;
                let Some(pattern) = s["pattern"].as_str() else {
                    continue;
                };
                matched += 1;
                let n = s["line"].as_u64().unwrap_or_else(|| panic!("no line: {l}")) as usize;
                let src_line = &modparser[n - 1];
                let base = match list {
                    "regenTypes" | "degenTypes" | "costTypes" | "baseCostTypes" => "resourceTypes",
                    other => other,
                };
                if s["gen"] == true {
                    // the generating statement writes into this table
                    assert!(
                        src_line.contains(&format!("{base}[")),
                        "{list} {pattern:?} -> ModParser.lua:{n} {src_line:?}"
                    );
                } else if list == "jewelFuncList" {
                    assert!(
                        src_line.to_lowercase().contains(&lua_key(pattern)),
                        "{list} {pattern:?} -> ModParser.lua:{n} {src_line:?}"
                    );
                } else {
                    let key = if list == "specialModList" {
                        // wrapped "^" .. k .. "$" (5949-5953) or added later
                        if src_line.contains(&lua_key(pattern)) {
                            pattern
                        } else {
                            &pattern[1..pattern.len() - 1]
                        }
                    } else {
                        pattern
                    };
                    assert!(
                        src_line.contains(&lua_key(key)),
                        "{list} {pattern:?} -> ModParser.lua:{n} {src_line:?}"
                    );
                }
            }
        }
        by_line.insert(line, r);
    }
    assert_eq!(by_line.len() as u64, records);
    eprintln!("{records} records, {steps} steps ({matched} matched), {with_fn} with closures; lists {lists:?}");
    assert!(with_fn > 0, "no jewel func records");
    // every corpus part contributed lines
    for label in [
        "modCache", "tree", "unique", "catalyst", "rare", "affix", "enchant", "cluster", "base",
        "legion", "foulborn", "pantheon",
    ] {
        assert!(labels.contains(label), "no record from {label}: {labels:?}");
    }
    for l in [
        "formList",
        "modNameList",
        "modTagList",
        "modFlagList",
        "preFlagList",
        "specialModList",
        "skillNameList",
        "preSkillNameList",
        "suffixTypes",
        "jewelFuncList",
        "clusterJewelSkills",
    ] {
        assert!(lists.contains_key(l), "no step over {l}");
    }

    // A known record (FORMAT.md's example).
    let r = &by_line["+10 to Strength"];
    assert_eq!(
        r["mods"],
        serde_json::json!([{"flags":0,"keywordFlags":0,"name":"Str","type":"BASE","value":10}])
    );
    let form = r["passes"][0]["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["list"] == "formList")
        .unwrap();
    assert_eq!(form["pattern"], "^([%+%-][%d%.]+)%%? to");
    assert_eq!(
        modparser[form["line"].as_u64().unwrap() as usize - 1],
        "\t[\"^([%+%-][%d%.]+)%%? to\"] = \"BASE\","
    );

    // PoB's own memo (Data/ModCache.lua, regenerated by PoB from the same
    // parser) agrees with every native parse, up to the precision
    // ModCache.lua is written with.
    let modcache: Value =
        serde_json::from_str(&std::fs::read_to_string(out.join("modCache.json")).unwrap()).unwrap();
    let (mut exact, mut close, mut differ) = (0, 0, Vec::new());
    for (line, e) in modcache.as_object().unwrap() {
        let r = by_line
            .get(line)
            .unwrap_or_else(|| panic!("ModCache line {line:?} missing"));
        if r["mods"] == e["mods"] && r["extra"] == e["extra"] {
            exact += 1;
        } else if same(&r["mods"], &e["mods"]) && r["extra"] == e["extra"] {
            close += 1;
        } else {
            differ.push(line.clone());
        }
    }
    eprintln!("ModCache: {exact} identical, {close} equal up to ModCache.lua's number precision");
    assert!(
        differ.is_empty(),
        "{} ModCache lines parse differently: {:?}",
        differ.len(),
        &differ[..differ.len().min(10)]
    );
    // the 14 `value=-0` of ModCache.lua survive the load and the parse
    let neg_zero = |v: &Value| v.to_string().contains(r#"{"$float":"-0"}"#);
    let cache_neg = modcache
        .as_object()
        .unwrap()
        .values()
        .filter(|e| neg_zero(e))
        .count();
    assert_eq!(cache_neg, 14, "modCache.json entries with -0");
    let r = &by_line["0% reduced Attack Speed"];
    assert!(neg_zero(&r["mods"]), "{}", r["mods"]);
    let r = &by_line["Debuffs on you expire 0% slower"];
    assert!(neg_zero(&r["mods"]), "{}", r["mods"]);
}

/// Two runs are byte-identical, and the scan prefilter changes nothing.
#[test]
fn deterministic_and_prefilter_equivalent() {
    let Some(src) = pob_src() else {
        eprintln!("skipping: PoB source not found (set POB_SRC)");
        return;
    };
    let parts: Vec<String> = ["modCache", "cluster", "pantheons", "bases", "legion"]
        .into_iter()
        .map(String::from)
        .collect();
    let opts = |prefilter| pob_dump::parse_oracle::Options {
        parts: Some(parts.clone()),
        mod_cache_stride: Some(7),
        prefilter,
    };
    let a = pob_dump::parse_oracle::run(&src, &opts(true)).unwrap();
    let b = pob_dump::parse_oracle::run(&src, &opts(true)).unwrap();
    let c = pob_dump::parse_oracle::run(&src, &opts(false)).unwrap();
    eprintln!("{} records", a.lines.len());
    assert!(a.lines.len() > 3_000);
    assert_eq!(a.info, b.info);
    assert!(a.bytes() == b.bytes(), "two runs differ");
    let diff = a.lines.iter().zip(&c.lines).find(|(x, y)| x != y);
    assert!(diff.is_none(), "prefilter changes a record: {diff:?}");
    assert_eq!(a.lines.len(), c.lines.len());
}
