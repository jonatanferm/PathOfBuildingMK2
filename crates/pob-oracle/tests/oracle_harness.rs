//! The oracle harness end to end: run the `pob_oracle` binary on one build
//! (fresh process per dump), deserialise both dumps, and check them against
//! each other and against PoB's known numbers for the build. Self-skips
//! without a PoB checkout (`$POB_SRC` or `.PathOfBuilding/src` in an ancestor)
//! or the OccVortex build; with `POB_ORACLE_REQUIRE=1` (or `CI`) it fails
//! instead, as U0.rs and u0_smoke.rs do.

use std::path::PathBuf;

use pob_oracle::legacy_modifier::Mod;
use pob_oracle::oracle::{find_pob_src, find_up, Leaf, Oracle};

fn build() -> Option<PathBuf> {
    let src = find_pob_src()?;
    find_up(".parity/builds/upstream_OccVortex.xml")
        .or_else(|| find_up(".PathOfBuilding/spec/TestBuilds/3.13/OccVortex.xml"))
        // `$POB_SRC` from a worktree outside the main checkout: the build sits
        // beside PoB's `src/` (spec/TestBuilds/3.13/OccVortex.xml is the file
        // .parity/builds/upstream_OccVortex.xml was copied from).
        .or_else(|| {
            let p = src.parent()?.join("spec/TestBuilds/3.13/OccVortex.xml");
            p.is_file().then_some(p)
        })
}

fn skip(why: &str) {
    let required = std::env::var_os("POB_ORACLE_REQUIRE").is_some_and(|v| v == "1")
        || std::env::var_os("CI").is_some_and(|v| !v.is_empty() && v != "false" && v != "0");
    assert!(!required, "required input missing: {why}");
    eprintln!("SKIPPED: {why}");
}

fn oracle() -> Oracle {
    let mut o = Oracle::with_bin(env!("CARGO_BIN_EXE_pob_oracle"));
    o.cache = None;
    o
}

#[test]
fn outputs_and_stages() {
    let Some(build) = build() else {
        skip("no PoB checkout (POB_SRC) or OccVortex build (.parity/builds, PoB spec/TestBuilds)");
        return;
    };
    let o = oracle();

    let t = std::time::Instant::now();
    let out = o.run_outputs(&build).unwrap_or_else(|e| panic!("{e}"));
    eprintln!("outputs: {:?}", t.elapsed());
    assert_eq!(out.format, pob_oracle::oracle::OUTPUTS_FORMAT);
    // CCR-0001/0002: interpreter only; numberMode follows jit.arch.
    let lj = out.luajit.as_ref().expect("luajit header");
    assert!(!lj.jit, "oracle parity runs use jit.off()");
    let single = matches!(lj.arch.as_str(), "x86" | "x64");
    assert_eq!(lj.number_mode, if single { "single" } else { "dual" }, "{lj:?}");
    // PoB's own expected values for this build (spec/TestBuilds/3.13/OccVortex.lua).
    assert_eq!(out.num("Life"), Some(6728.0));
    let main = out.main_skill.as_ref().expect("main skill");
    assert_eq!(main.id, "Vortex");
    let total_dps = out.num("TotalDPS").expect("TotalDPS");
    assert!(total_dps > 1000.0);
    // Types and nesting survive: booleans, strings, nested tables.
    assert!(out.player.values().any(serde_json::Value::is_boolean));
    assert!(out.player.values().any(serde_json::Value::is_object));
    let flat = out.flat();
    assert!(flat.len() > 500, "{} leaves", flat.len());
    assert!(flat.values().any(|l| matches!(l, Leaf::Str(_))));
    assert_eq!(
        out.get("ReqDexItem.source")
            .and_then(serde_json::Value::as_str),
        flat.get("ReqDexItem.source").and_then(|l| match l {
            Leaf::Str(s) => Some(s.as_str()),
            _ => None,
        })
    );

    let t = std::time::Instant::now();
    let st = o.run_stages(&build).unwrap_or_else(|e| panic!("{e}"));
    eprintln!("stages: {:?}", t.elapsed());
    assert_eq!(st.format, pob_oracle::oracle::STAGES_FORMAT);
    assert!(st.main_env_is_captured);
    let ie = &st.init_env;
    assert_eq!(ie.mode, "MAIN");
    let mods = &ie.player.mod_db.mods;
    assert!(mods.len() > 100, "{} player mods", mods.len());
    // Base life per level, as CalcSetup adds it.
    assert!(ie
        .player
        .mod_db
        .named("Life")
        .any(|m| m.source.as_deref() == Some("Base") && !m.tags.is_empty()));
    assert!(!ie.enemy.mod_db.mods.is_empty());
    assert!(!ie.player.mod_db.conditions.is_empty());
    assert!(!ie.player.mod_db.multipliers.is_empty());
    let main_stage = st.main_skill().expect("main skill stage");
    assert_eq!(main_stage.id, "Vortex");
    assert!(!main_stage.skill_types.is_empty());
    assert!(!main_stage.skill_mod_list.mods.is_empty());
    let cfg = main_stage.skill_cfg.as_ref().expect("skillCfg");
    assert_eq!(cfg.skill_name.as_deref(), Some("Vortex"));
    assert_eq!(cfg.skill_types, main_stage.skill_types);

    // The main skill's per-skill output is the main env's (offence; the
    // per-skill pass skips EHP like calcFullDPS).
    let so = st
        .skill_outputs
        .iter()
        .find(|s| Some(s.index) == ie.main_skill_index)
        .expect("main skill output");
    assert!(so.error.is_none(), "{:?}", so.error);
    let so_dps = so
        .output
        .as_ref()
        .and_then(|o| o.get("TotalDPS"))
        .and_then(serde_json::Value::as_f64);
    assert_eq!(so_dps, Some(total_dps));
    assert!(st.skill_outputs.len() >= 5);
}

/// Every mod PoB holds round-trips through `pob_oracle::legacy_modifier::Mod`'s serde
/// shape unchanged (modulo fields PoB set to false / nil, which the typed
/// model doesn't distinguish from absent).
#[test]
fn stage_mods_round_trip() {
    let Some(build) = build() else {
        skip("no PoB checkout (POB_SRC) or OccVortex build (.parity/builds, PoB spec/TestBuilds)");
        return;
    };
    let raw = oracle()
        .run(pob_oracle::oracle::Dump::Stages, &build)
        .unwrap_or_else(|e| panic!("{e}"));
    let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let mut mods = Vec::new();
    collect_mods(&v["initEnv"], &mut mods);
    assert!(mods.len() > 500, "{} mods", mods.len());
    let mut bad = Vec::new();
    for m in &mods {
        let typed: Mod =
            serde_json::from_value((*m).clone()).unwrap_or_else(|e| panic!("{e}: {m}"));
        let back = serde_json::to_value(&typed).unwrap();
        if normalize(&back) != normalize(m) {
            bad.push(format!("{m}\n  -> {back}"));
        }
    }
    for b in bad.iter().take(10) {
        eprintln!("{b}");
    }
    assert!(
        bad.is_empty(),
        "{} of {} mods changed in a round trip",
        bad.len(),
        mods.len()
    );
}

fn collect_mods<'a>(v: &'a serde_json::Value, out: &mut Vec<&'a serde_json::Value>) {
    match v {
        serde_json::Value::Object(m) => {
            if let Some(serde_json::Value::Array(list)) = m.get("mods") {
                out.extend(list.iter());
            }
            for (k, x) in m {
                if k != "mods" {
                    collect_mods(x, out);
                }
            }
        }
        serde_json::Value::Array(a) => a.iter().for_each(|x| collect_mods(x, out)),
        _ => {}
    }
}

/// Drop `false` / `null` fields and zero tag flags, which the typed model
/// treats as unset.
fn normalize(v: &serde_json::Value) -> serde_json::Value {
    use serde_json::Value as J;
    match v {
        J::Object(m) => J::Object(
            m.iter()
                .filter(|(k, x)| {
                    !(x.is_null()
                        || **x == J::Bool(false)
                        || ((k.as_str() == "modFlags" || k.as_str() == "keywordFlags")
                            && x.as_f64() == Some(0.0)
                            && !m.contains_key("name")))
                })
                .map(|(k, x)| (k.clone(), normalize(x)))
                .collect(),
        ),
        J::Array(a) => J::Array(a.iter().map(normalize).collect()),
        J::Number(n) => n
            .as_f64()
            .and_then(serde_json::Number::from_f64)
            .map_or(J::Number(n.clone()), J::Number),
        other => other.clone(),
    }
}
