//! Full dump of PoB's data, checked for counts, closure handling and
//! self-consistency of every `$ref`.
//!
//! Needs PoB's source (`pob_testpaths`: `$POB_SRC`, this workspace, the main
//! checkout). Missing, the tests fail, unless `POB_ALLOW_SKIP=1`.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use serde_json::Value;

#[rustfmt::skip]
#[path = "../generated/enums.rs"]
mod enums;
#[rustfmt::skip]
#[path = "../generated/misc.rs"]
mod misc;

#[test]
fn generated_enums() {
    use enums::{keyword_flag, mod_flag, skill_type};
    assert_eq!(skill_type::Area, 8);
    assert_eq!(skill_type::Aura, 39);
    assert_eq!(skill_type::Pact, 135);
    assert_eq!(skill_type::MAX, 135);
    assert_eq!(skill_type::name(39), Some("Aura"));
    assert_eq!(skill_type::from_name("Pact"), Some(135));
    assert_eq!(mod_flag::Attack, 1);
    assert_eq!(mod_flag::WeaponMask, 0x2FFF_0000);
    assert_eq!(keyword_flag::MatchAll, 0x4000_0000);
    assert_eq!(keyword_flag::names(0x21), vec!["Aura", "Fire"]);
}

/// PoB's `src/` via `pob_testpaths` (`$POB_SRC`, this workspace, the main
/// checkout). Missing: the test fails, or returns with `POB_ALLOW_SKIP=1`.
fn pob_src() -> Option<PathBuf> {
    pob_testpaths::require(
        pob_testpaths::pob_src(),
        "PoB checkout (.PathOfBuilding/src)",
    )
}

fn load(path: &Path) -> Value {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// One step of a dump path.
#[derive(Debug, Clone)]
enum Step {
    Key(String),
    Index(String),
    Meta,
}

/// Parse the part of a path after its root.
fn parse_steps(mut s: &str) -> Vec<Step> {
    let mut out = Vec::new();
    while !s.is_empty() {
        if let Some(rest) = s.strip_prefix("<meta>") {
            out.push(Step::Meta);
            s = rest;
        } else if let Some(rest) = s.strip_prefix('.') {
            let end = rest.find(['.', '[', '<']).unwrap_or(rest.len());
            out.push(Step::Key(rest[..end].to_owned()));
            s = &rest[end..];
        } else if let Some(rest) = s.strip_prefix("[\"") {
            // JSON string literal up to the unescaped closing quote
            let bytes = rest.as_bytes();
            let mut i = 0;
            while bytes[i] != b'"' {
                i += if bytes[i] == b'\\' { 2 } else { 1 };
            }
            let lit = format!("\"{}\"", &rest[..i]);
            out.push(Step::Key(serde_json::from_str(&lit).unwrap()));
            s = &rest[i + 2..];
        } else if let Some(rest) = s.strip_prefix('[') {
            let end = rest.find(']').unwrap();
            let k = &rest[..end];
            assert!(
                !k.starts_with('@') && !k.starts_with('$') && k != "true" && k != "false",
                "path step [{k}] not supported by this test"
            );
            out.push(Step::Index(k.to_owned()));
            s = &rest[end + 1..];
        } else {
            panic!("bad path rest {s:?}");
        }
    }
    out
}

fn navigate<'a>(mut v: &'a Value, steps: &[Step], full: &str) -> &'a Value {
    for st in steps {
        v = match st {
            Step::Meta => &v["$meta"],
            Step::Key(k) => {
                let k = if k.starts_with('$') {
                    format!("${k}")
                } else {
                    k.clone()
                };
                &v[k.as_str()]
            }
            Step::Index(n) => match v {
                Value::Array(a) => &a[n.parse::<usize>().unwrap() - 1],
                Value::Object(o) => {
                    let seq_len = o.get("$seq").and_then(Value::as_array).map_or(0, Vec::len);
                    match n.parse::<usize>() {
                        Ok(i) if i >= 1 && i <= seq_len => &o["$seq"][i - 1],
                        _ => &o["$num"][n.as_str()],
                    }
                }
                _ => panic!("{full}: indexing a scalar"),
            },
        };
        assert!(!v.is_null(), "{full}: step {st:?} not found");
    }
    v
}

#[derive(Default)]
struct Scan {
    fn_ids: BTreeSet<String>,
    refs: Vec<String>,
    tables: usize,
}

fn scan(v: &Value, s: &mut Scan, file: &str) {
    match v {
        Value::Object(o) => {
            if let Some(id) = o.get("$fn") {
                let keys: BTreeSet<&str> = o.keys().map(String::as_str).collect();
                assert_eq!(
                    keys,
                    BTreeSet::from(["$fn", "src", "hash"]),
                    "{file}: bad $fn object"
                );
                let h = o["hash"].as_str().unwrap();
                assert!(
                    h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit()),
                    "{file}: bad hash"
                );
                s.fn_ids.insert(id.as_str().unwrap().to_owned());
                return;
            }
            if let Some(r) = o.get("$ref") {
                assert_eq!(o.len(), 1, "{file}: bad $ref object");
                s.refs.push(r.as_str().unwrap().to_owned());
                return;
            }
            s.tables += 1;
            for x in o.values() {
                scan(x, s, file);
            }
        }
        Value::Array(a) => {
            s.tables += 1;
            for x in a {
                scan(x, s, file);
            }
        }
        Value::String(st) => {
            assert!(
                !st.starts_with("function: 0x") && !st.starts_with("function: builtin"),
                "{file}: raw function value {st}"
            );
        }
        _ => {}
    }
}

#[test]
fn full_dump() {
    let Some(src) = pob_src() else {
        return; // POB_ALLOW_SKIP=1
    };
    // The committed enums must match what Global.lua produces today.
    let generated = pob_dump::codegen::codegen_enums(&src).unwrap();
    let committed = include_str!("../generated/enums.rs");
    // (whitespace-insensitive, so rustfmt on the committed copy is harmless)
    let squash = |s: &str| s.split_whitespace().collect::<String>();
    assert!(
        squash(&generated) == squash(committed),
        "crates/pob-dump/generated/enums.rs is stale; rerun pob-dump --codegen-enums"
    );

    let out = Path::new(env!("CARGO_TARGET_TMPDIR")).join("pob-dump-test");
    let report = pob_dump::dump(&pob_dump::Options {
        pob_src: src.clone(),
        out: out.clone(),
        pretty: true,
        skip_trees: false,
        parse_oracle: false,
    })
    .unwrap();
    eprintln!(
        "dump: {} files, {} bytes, timings {:?}",
        report.files.len(),
        report.total_bytes(),
        report.timings
    );
    assert!(
        report.skipped.is_empty(),
        "skipped files: {:?}",
        report.skipped
    );
    assert_eq!(
        report.stats.get("opaque"),
        Some(&0),
        "userdata/thread values in data"
    );

    // --- counts -------------------------------------------------------
    let skills = load(&out.join("skills.json"));
    assert_eq!(skills.as_object().unwrap().len(), 1526, "granted effects");
    let gems = load(&out.join("gems.json"));
    let gems = gems.as_object().unwrap();
    let gems_lua = std::fs::read_to_string(src.join("Data/Gems.lua")).unwrap();
    let raw_ids: Vec<&str> = gems_lua
        .lines()
        .filter_map(|l| l.strip_prefix("\t[\"").and_then(|r| r.split('"').next()))
        .collect();
    assert_eq!(raw_ids.len(), 822, "gems in Data/Gems.lua");
    for id in &raw_ids {
        assert!(gems.contains_key(*id), "gem {id} missing");
    }
    // Data.lua's setupGem adds the Vaal AltX/AltY variants.
    let synthesized: Vec<&String> = gems
        .keys()
        .filter(|k| !raw_ids.contains(&k.as_str()))
        .collect();
    for id in &synthesized {
        let g = &gems[id.as_str()];
        assert_eq!(g["vaalGem"], Value::Bool(true), "{id}");
        assert!(id.ends_with("AltX") || id.ends_with("AltY"), "{id}");
        assert_eq!(
            g["grantedEffect"]["$ref"],
            format!("data.skills.{}", g["grantedEffectId"].as_str().unwrap())
        );
    }
    assert_eq!(
        gems.len(),
        871,
        "gems after Data.lua (822 + {} Vaal alts)",
        synthesized.len()
    );
    // statMap metatable recorded
    assert_eq!(
        skills["Fireball"]["statMap"]["$meta"]["$ref"],
        "data.skillStatMapMeta"
    );

    // The 14 `value=-0` literals of Data/ModCache.lua are -0, as on x86-64
    // PoB, also on a dual-number LuaJIT (prelude.lua restores them).
    let modcache = load(&out.join("modCache.json"));
    let neg_zero = |v: &Value| v.to_string().contains(r#"{"$float":"-0"}"#);
    let neg_entries = modcache
        .as_object()
        .unwrap()
        .values()
        .filter(|e| neg_zero(e))
        .count();
    assert_eq!(neg_entries, 14, "modCache.json entries with -0");
    assert!(neg_zero(&modcache["0% reduced Attack Speed"]));
    if let Some(&rewrites) = report.stats.get("negZeroLiterals:Data/ModCache.lua") {
        assert_eq!(rewrites, 14);
        assert_eq!(
            report
                .stats
                .keys()
                .filter(|k| k.starts_with("negZeroLiterals:"))
                .count(),
            1
        );
    }

    // misc.json: Data/Misc.lua keys, non-table values, and data.misc (whose
    // own file name would collide with misc.json); the generated record
    // structs deserialise from it.
    let misc_json = load(&out.join("misc.json"));
    assert_eq!(misc_json["misc"]["ServerTickTime"], 0.033);
    let m: misc::misc = serde_json::from_value(misc_json["misc"].clone()).unwrap();
    assert_eq!(m, misc::misc::PINNED);
    let c: misc::characterConstants =
        serde_json::from_value(misc_json["characterConstants"].clone()).unwrap();
    assert_eq!(c, misc::characterConstants::PINNED);
    let c: misc::monsterConstants =
        serde_json::from_value(misc_json["monsterConstants"].clone()).unwrap();
    assert_eq!(c, misc::monsterConstants::PINNED);
    let c: misc::gameConstants =
        serde_json::from_value(misc_json["gameConstants"].clone()).unwrap();
    assert_eq!(c, misc::gameConstants::PINNED);

    let global = load(&out.join("global.json"));
    assert_eq!(global["SkillType"]["Area"], 8);
    assert_eq!(global["SkillType"]["Aura"], 39);
    assert_eq!(global["SkillType"]["Pact"], 135);
    assert_eq!(global["ModFlag"]["Attack"], 1);
    assert_eq!(global["KeywordFlag"]["MatchAll"], 0x4000_0000);

    let modcache_lua = std::fs::read_to_string(src.join("Data/ModCache.lua")).unwrap();
    let cache_lines = modcache_lua.lines().filter(|l| l.starts_with("c[")).count();
    assert_eq!(cache_lines, 23207);
    let modcache = load(&out.join("modCache.json"));
    assert_eq!(modcache.as_object().unwrap().len(), cache_lines);
    assert_eq!(report.modcache_entries, cache_lines);

    // every tree version PoB knows, plus every tree.lua on disk
    let gv = load(&out.join("gameVersions.json"));
    let versions: Vec<&str> = gv["treeVersionList"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(versions.len(), 41);
    for v in &versions {
        assert!(out.join(format!("trees/{v}.json")).exists(), "tree {v}");
        if src.join(format!("TreeData/{v}/sprites.lua")).exists() {
            assert!(
                out.join(format!("trees/{v}.sprites.json")).exists(),
                "sprites {v}"
            );
        }
    }
    assert!(out.join("trees/legion.json").exists());
    assert!(out.join("trees/3_19.assets.json").exists());

    // --- closures -----------------------------------------------------
    let closures = load(&out.join("closures.json"));
    let fams: BTreeMap<String, usize> =
        serde_json::from_value(closures["families"].clone()).unwrap();
    let skill_hooks: usize = fams
        .iter()
        .filter(|(k, _)| k.starts_with("skill-hook:"))
        .map(|(_, v)| v)
        .sum();
    assert_eq!(skill_hooks, 133, "skill hook closures: {fams:?}");
    assert_eq!(fams.get("skill-hook:preDamageFunc"), Some(&122));
    assert_eq!(fams.get("modmap:apply"), Some(&41));
    assert_eq!(fams.get("config:apply"), Some(&532));
    let manifest_ids: BTreeSet<String> = closures["closures"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        manifest_ids.len(),
        closures["count"].as_u64().unwrap() as usize
    );

    // --- no raw functions; every $fn listed; every $ref resolves --------
    let manifest = load(&out.join("manifest.json"));
    let mut root_file: Vec<(String, String, bool)> = Vec::new(); // (root path, file, grouped)
    for f in manifest["files"].as_array().unwrap() {
        let file = f["file"].as_str().unwrap().to_owned();
        let roots = f["roots"].as_array().unwrap();
        for r in roots {
            root_file.push((
                r.as_str().unwrap().to_owned(),
                file.clone(),
                file == "misc.json",
            ));
        }
    }
    root_file.sort_by_key(|r| std::cmp::Reverse(r.0.len()));
    let mut all = Scan::default();
    for f in manifest["files"].as_array().unwrap() {
        let file = f["file"].as_str().unwrap();
        if file == "closures.json" {
            continue;
        }
        let v = load(&out.join(file));
        let mut s = Scan::default();
        scan(&v, &mut s, file);
        all.fn_ids.extend(s.fn_ids);
        all.refs.extend(s.refs);
        all.tables += s.tables;
    }
    assert_eq!(
        all.fn_ids, manifest_ids,
        "$fn ids in files vs closures.json"
    );

    // group refs by target file
    // target file -> [(full path, misc.json member, steps)]
    type Pending = Vec<(String, Option<String>, Vec<Step>)>;
    let mut by_file: HashMap<String, Pending> = HashMap::new();
    for r in &all.refs {
        let (root, file, grouped) = root_file
            .iter()
            .find(|(root, _, _)| {
                r.starts_with(root.as_str())
                    && matches!(
                        r.as_bytes().get(root.len()),
                        None | Some(b'.' | b'[' | b'<')
                    )
            })
            .unwrap_or_else(|| panic!("$ref {r} has no file"));
        let key = grouped.then(|| root.rsplit('.').next().unwrap().to_owned());
        by_file.entry(file.clone()).or_default().push((
            r.clone(),
            key,
            parse_steps(&r[root.len()..]),
        ));
    }
    let mut resolved = 0;
    for (file, refs) in by_file {
        let v = load(&out.join(&file));
        for (full, key, steps) in refs {
            let base = match &key {
                Some(k) => &v[k.as_str()],
                None => &v,
            };
            let target = navigate(base, &steps, &full);
            assert!(
                target.is_object() || target.is_array(),
                "$ref {full} resolves to a scalar"
            );
            assert!(
                target.get("$ref").is_none(),
                "$ref {full} resolves to another $ref"
            );
            resolved += 1;
        }
    }
    eprintln!(
        "checked {} tables, {} $fn ids, {resolved} $refs",
        all.tables,
        all.fn_ids.len()
    );
    assert!(resolved > 30_000);

    // --- determinism: a second dump is byte-identical, manifest included ---
    let out2 = Path::new(env!("CARGO_TARGET_TMPDIR")).join("pob-dump-test-2");
    let report2 = pob_dump::dump(&pob_dump::Options {
        pob_src: src,
        out: out2.clone(),
        pretty: true,
        skip_trees: false,
        parse_oracle: false,
    })
    .unwrap();
    assert_eq!(report.files.len(), report2.files.len());
    let mut compared = 0;
    for f in &report.files {
        let a = std::fs::read(out.join(&f.name)).unwrap();
        let b = std::fs::read(out2.join(&f.name)).unwrap();
        assert!(a == b, "{} differs between two dumps", f.name);
        compared += 1;
    }
    eprintln!("determinism: {compared} files byte-identical across two dumps");
}
