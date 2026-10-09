//! Unit U0 acceptance (calc doc §11.1, row U0):
//!
//! * `compile_zero_unknown_keys`: the real dump deserialises into the strict
//!   schema (`pob_gamedata::Data`, `deny_unknown_fields` everywhere, every
//!   dump root typed or listed as dynamic) and `data.bin` round-trips; the
//!   runtime load of `data.bin` is timed (< 1 s gate in optimised builds).
//! * `registry_complete`: every closures.json id of the registered families
//!   (skill hooks, ConfigOptions apply, ModMap apply) has exactly one registry
//!   row, with the same sha256.
//! * `stages_patch_loads`: the patched oracle with every `docs/port/stages.toml`
//!   stage plans and passes the `loadstring` validation.
//! * `stages_fire_on_slice5` (opt-in, `POB_ORACLE_PROBE=1`): the patched oracle
//!   runs a slice5 build and every stage id fires (`hits.json`). It runs the
//!   release `pob_oracle` binary (`POB_ORACLE_BIN`, else `target/release/pob_oracle`;
//!   build it first with `scripts/port/cargo.sh build --release -p pob-oracle --bin pob_oracle`).
//! * `provenance_check_and_coverage`: `scripts/port/provenance.py check` and
//!   `coverage` (docs/port/scope.toml) are clean.
//!
//! `buildOutput` end to end on stubs is `tests/u0_smoke.rs`.
//!
//! A test whose inputs (data/dump, the PoB checkout) are absent skips with a
//! message; with `POB_ORACLE_REQUIRE=1` (or `CI`) it fails instead.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::process::Command;
use std::time::Instant;

use pob_oracle::oracle::{find_pob_src, find_up};
use pob_oracle::patcher::{self, StagesFile};

fn skip(why: &str) {
    let required = std::env::var_os("POB_ORACLE_REQUIRE").is_some_and(|v| v == "1")
        || std::env::var_os("CI").is_some_and(|v| !v.is_empty() && v != "false" && v != "0");
    assert!(!required, "required input missing: {why}");
    eprintln!("SKIPPED: {why}");
}

fn dump_dir() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("POB_DUMP_DIR").map(PathBuf::from) {
        return p.join("manifest.json").is_file().then_some(p);
    }
    find_up("data/dump/manifest.json").map(|p| p.parent().unwrap().to_path_buf())
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn compile_zero_unknown_keys() {
    let Some(dir) = dump_dir() else {
        return skip("data/dump not found (run pob-dump)");
    };
    let t0 = Instant::now();
    let tree = pob_gamedata::load_dump_dir(&dir).unwrap_or_else(|e| panic!("load dump: {e}"));
    let t_json = t0.elapsed();
    let data = pob_gamedata::Data::from_tree(&tree)
        .unwrap_or_else(|e| panic!("schema (unknown key or root): {e}"));
    // Counts the docs pin (calc doc §13, AMENDMENTS A2).
    assert_eq!(data.gems.len(), 871, "gems (822 + 49 Vaal AltX/AltY)");
    assert!(data.skills.len() > 1500, "skills: {}", data.skills.len());
    assert_eq!(data.modCache.len(), 23207, "ModCache lines");
    assert_eq!(data.gameVersions.treeVersionList.len(), 41, "tree versions");
    assert_eq!(pob_gamedata::SkillType::Area, 8);
    assert_eq!(pob_gamedata::SkillType::Aura, 39);
    assert_eq!(pob_gamedata::SkillType::Pact, 135);
    // shared Arcs follow Lua identity: a gem's grantedEffect is the data.skills entry
    let fb = &data.gems[&pob_gamedata::Name::from("Metadata/Items/Gems/SkillGemFireball")];
    assert!(std::sync::Arc::ptr_eq(
        &fb.grantedEffect,
        &data.skills[&pob_gamedata::Name::from("Fireball")]
    ));
    // the calc-read roots CCR-0011 §7 types (they are not in `Data::other`)
    {
        use pob_gamedata::schema::item_data::EnchantmentList;
        use pob_gamedata::Name;
        let e = &data.enchantments;
        assert!(std::sync::Arc::ptr_eq(
            e.get("UtilityFlask").unwrap(),
            e.get("Flask").unwrap()
        ));
        assert!(matches!(
            e.get("Helmet").unwrap()[&Name::from("Arc")],
            EnchantmentList::BySource(_)
        ));
        assert!(matches!(
            e.get("Claw").unwrap()[&Name::from("HARVEST")],
            EnchantmentList::Lines(_)
        ));
        assert_eq!(data.foulbornMap.0.len(), 248);
        assert_eq!(data.rareLikeUniques.0.len(), 8);
        assert!(data
            .casterTagCrucibleUniques
            .contains(&Name::from("Mjolner")));
        assert_eq!(data.bossStats.UberArmourMean, 125.0);
        assert_eq!(data.nodeIDList.nodes.len(), 1937);
        assert_eq!(data.nodeIDList.size, 1937.0);
        assert_eq!(data.powerStatList.stats.len(), 85);
        assert!(data.modScalability.len() > 15000);
        let fb = &data.gemsByGameId.0[&Name::from("Metadata/Items/Gems/SkillGemFireball")];
        assert!(fb
            .values()
            .any(|g| std::sync::Arc::ptr_eq(g, &data.gems[&g.id])));
        for root in pob_gamedata::data::DYNAMIC_ROOTS {
            assert!(
                !pob_gamedata::data::TYPED_ROOTS.contains(root),
                "{root} both typed and dynamic"
            );
        }
    }
    eprintln!(
        "U0 compile: {} roots ({} dynamic), {} skills, {} gems, json load {:.2}s",
        tree.roots.len(),
        data.other.len(),
        data.skills.len(),
        data.gems.len(),
        t_json.as_secs_f64()
    );
    drop(data);

    // data.bin round trip, timed as the runtime loads it
    let bytes = pob_gamedata::loader::encode_bin(&tree).unwrap();
    drop(tree);
    let t1 = Instant::now();
    let tree2 = pob_gamedata::loader::decode_bin(&bytes).unwrap();
    let data2 = pob_gamedata::Data::from_tree(&tree2).unwrap();
    let t_load = t1.elapsed().as_secs_f64();
    assert_eq!(data2.gems.len(), 871);
    eprintln!(
        "U0 data.bin: {:.1} MB, runtime load {:.3}s",
        bytes.len() as f64 / 1e6,
        t_load
    );
    if !cfg!(debug_assertions) {
        assert!(
            t_load < 1.0,
            "data.bin load took {t_load:.3}s (gate: < 1 s)"
        );
    }
}

/// An unknown key fails the load and names its Lua path.
#[test]
fn unknown_key_is_an_error() {
    let mut t = pob_gamedata::loader::DumpTree::default();
    let v = pob_gamedata::loader::parse_json(
        r#"{"x":{"id":"x","name":"X","levels":[],"modSource":"Skill:x","notAKey":1}}"#,
    )
    .unwrap();
    t.add_file(&["data.skills".into()], v).unwrap();
    let e = t
        .get::<indexmap::IndexMap<String, pob_gamedata::GrantedEffect>>("data.skills")
        .expect_err("unknown key must fail");
    assert!(e.to_string().contains("data.skills.x.notAKey"), "{e}");
}

#[test]
fn registry_complete() {
    let Some(dir) = dump_dir() else {
        return skip("data/dump not found");
    };
    let text = std::fs::read_to_string(dir.join("closures.json")).unwrap();
    let j: serde_json::Value = serde_json::from_str(&text).unwrap();
    let mut want: BTreeMap<&str, BTreeMap<String, String>> = BTreeMap::new();
    for c in j["closures"].as_array().unwrap() {
        let fam = c["family"].as_str().unwrap();
        let group = if fam.starts_with("skill-hook:") {
            "skill"
        } else if fam == "config:apply" {
            "config"
        } else if fam == "modmap:apply" {
            "modmap"
        } else {
            continue;
        };
        want.entry(group).or_default().insert(
            c["id"].as_str().unwrap().to_owned(),
            c["hash"].as_str().unwrap().to_owned(),
        );
    }
    let mut have: BTreeMap<&str, BTreeMap<String, String>> = BTreeMap::new();
    let mut dup = Vec::new();
    for (g, rows) in [
        (
            "skill",
            pob_calc::data_fns::SKILL_FUNCS
                .values()
                .map(|r| (r.id, r.hash))
                .collect::<Vec<_>>(),
        ),
        (
            "config",
            pob_calc::modules::config_options::CONFIG_APPLY
                .values()
                .map(|r| (r.id, r.hash))
                .collect(),
        ),
        (
            "modmap",
            pob_calc::modules::mod_map::MODMAP_APPLY
                .values()
                .map(|r| (r.id, r.hash))
                .collect(),
        ),
    ] {
        for (id, h) in rows {
            if have
                .entry(g)
                .or_default()
                .insert(id.to_owned(), h.to_owned())
                .is_some()
            {
                dup.push(id);
            }
        }
    }
    // the registries' row counts (maps dedupe ids, so compare with the static row lists too)
    let skill_rows = pob_calc::data_fns::skills_act_int::ROWS.len()
        + pob_calc::data_fns::skills_act_dex::ROWS.len()
        + pob_calc::data_fns::skills_act_str::ROWS.len()
        + pob_calc::data_fns::skills_misc::ROWS.len();
    let config_rows = pob_calc::modules::config_options::apply_a::ROWS.len()
        + pob_calc::modules::config_options::apply_b::ROWS.len();
    let modmap_rows = pob_calc::modules::mod_map::ROWS.len();
    for (g, n) in [
        ("skill", skill_rows),
        ("config", config_rows),
        ("modmap", modmap_rows),
    ] {
        assert_eq!(
            n,
            want[g].len(),
            "{g}: {n} rows, manifest has {}",
            want[g].len()
        );
    }
    assert!(dup.is_empty(), "duplicate registry ids: {dup:?}");
    for g in ["skill", "config", "modmap"] {
        let w = &want[g];
        let h = &have[g];
        let missing: BTreeSet<_> = w.keys().filter(|k| !h.contains_key(*k)).collect();
        let extra: BTreeSet<_> = h.keys().filter(|k| !w.contains_key(*k)).collect();
        let changed: BTreeSet<_> = w
            .iter()
            .filter(|(k, v)| h.get(*k).is_some_and(|x| x != *v))
            .map(|(k, _)| k)
            .collect();
        assert!(
            missing.is_empty() && extra.is_empty() && changed.is_empty(),
            "{g}: missing {missing:?}, extra {extra:?}, hash changed {changed:?}"
        );
        eprintln!(
            "registry {g}: {} ids, all present with matching hashes",
            w.len()
        );
    }
    assert_eq!(want["skill"].len(), 133);
    assert_eq!(want["config"].len(), 532);
    assert_eq!(want["modmap"].len(), 41);
}

#[test]
fn stages_patch_loads() {
    let Some(src) = find_pob_src() else {
        return skip("PoB checkout (.PathOfBuilding/src) not found");
    };
    let stages_path = repo_root().join("docs/port/stages.toml");
    let stages = StagesFile::load(&stages_path).unwrap_or_else(|e| panic!("{e}"));
    assert!(stages.stage.len() >= 32, "stages: {}", stages.stage.len());
    let plan = patcher::plan(&src, &stages).unwrap_or_else(|e| panic!("plan: {e}"));
    patcher::validate(&plan).unwrap_or_else(|e| panic!("loadstring validation: {e}"));
    let line_sites = plan
        .sites
        .iter()
        .filter(|s| s.kind == patcher::SiteKind::Line)
        .count();
    assert_eq!(line_sites, stages.stage.len());
    eprintln!(
        "stages.toml: {} line probes, {} sites in total, {} patched files",
        line_sites,
        plan.sites.len(),
        plan.files.len()
    );
}

#[test]
fn stages_fire_on_slice5() {
    if std::env::var_os("POB_ORACLE_PROBE").is_none_or(|v| v != "1") {
        eprintln!("SKIPPED: opt-in (POB_ORACLE_PROBE=1)");
        return;
    }
    let Some(src) = find_pob_src() else {
        return skip("PoB checkout (.PathOfBuilding/src) not found");
    };
    let root = repo_root();
    let bin = std::env::var_os("POB_ORACLE_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("CARGO_TARGET_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| root.join("target"))
                .join("release/pob_oracle")
        });
    assert!(
        bin.is_file(),
        "{} not found: build it with scripts/port/cargo.sh build --release -p pob-oracle --bin pob_oracle",
        bin.display()
    );
    let stages_path = root.join("docs/port/stages.toml");
    let stages = StagesFile::load(&stages_path).unwrap_or_else(|e| panic!("{e}"));
    let build = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("corpus/U0/slice5/slice5_allflame_exp_031.xml");
    let out = std::env::temp_dir().join(format!("u0-stages-probe-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&out);
    let status = Command::new(&bin)
        .arg("probe")
        .arg(&build)
        .arg("--out-dir")
        .arg(&out)
        .arg("--stages")
        .arg(&stages_path)
        .arg("--pob")
        .arg(&src)
        .status()
        .expect("pob_oracle");
    assert!(status.success(), "pob_oracle probe failed");
    let hits: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(out.join("hits.json")).unwrap()).unwrap();
    let counts = hits["counts"].as_object().expect("hits.json counts");
    let missing: Vec<&str> = stages
        .stage
        .iter()
        .map(|s| s.id.as_str())
        .filter(|id| {
            counts
                .get(*id)
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0)
                == 0
        })
        .collect();
    let _ = std::fs::remove_dir_all(&out);
    assert!(
        missing.is_empty(),
        "stage ids that never fired: {missing:?}"
    );
    eprintln!(
        "stages_fire_on_slice5: all {} stage ids fired",
        stages.stage.len()
    );
}

#[test]
fn provenance_check_and_coverage() {
    if find_pob_src().is_none() {
        return skip("PoB checkout not found");
    }
    let root = repo_root();
    for args in [vec!["check"], vec!["coverage"]] {
        let out = Command::new("python3")
            .arg(root.join("scripts/port/provenance.py"))
            .args(&args)
            .current_dir(&root)
            .output()
            .expect("python3");
        let text = String::from_utf8_lossy(&out.stdout);
        let last = text.lines().last().unwrap_or_default().to_owned();
        eprintln!("provenance {}: {last}", args[0]);
        assert!(
            out.status.success(),
            "provenance {} failed:\n{text}{}",
            args[0],
            String::from_utf8_lossy(&out.stderr)
        );
    }
}
