//! Unit U0 acceptance (calc doc §11.1, row U0):
//!
//! * `compile_zero_unknown_keys` and `unknown_key_is_an_error` need no PoB: GOLD1 moved them to
//!   `crates/pob-gamedata/tests/dump_load.rs` (docs/standalone/GOLDEN-PLAN.md §1.7), where the
//!   first starts from `data.bin` instead of the JSON dump.
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
//! The inputs (data/dump, the PoB checkout) are found by `pob_testpaths`; a
//! missing input fails the test unless `POB_ALLOW_SKIP=1`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::process::Command;

use pob_oracle::patcher::{self, StagesFile};

/// `data/dump` (missing: fail, or `None` with `POB_ALLOW_SKIP=1`).
fn dump_dir() -> Option<PathBuf> {
    pob_testpaths::require(pob_testpaths::dump_dir(), "data/dump (run pob-dump)")
}

/// PoB's `src/` (missing: fail, or `None` with `POB_ALLOW_SKIP=1`).
fn pob_src() -> Option<PathBuf> {
    pob_testpaths::require(
        pob_testpaths::pob_src(),
        "PoB checkout (.PathOfBuilding/src)",
    )
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn registry_complete() {
    let Some(dir) = dump_dir() else {
        return; // POB_ALLOW_SKIP=1
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
    let Some(src) = pob_src() else {
        return; // POB_ALLOW_SKIP=1
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
    let Some(src) = pob_src() else {
        return; // POB_ALLOW_SKIP=1
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
    let Some(src) = pob_src() else {
        return; // POB_ALLOW_SKIP=1
    };
    let root = repo_root();
    for args in [vec!["check"], vec!["coverage"]] {
        let out = Command::new("python3")
            .arg(root.join("scripts/port/provenance.py"))
            .args(&args)
            .current_dir(&root)
            // the checkout pob_testpaths found (scripts/port/pobpaths.py reads $POB_DIR)
            .env("POB_DIR", src.parent().expect("PoB root"))
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
