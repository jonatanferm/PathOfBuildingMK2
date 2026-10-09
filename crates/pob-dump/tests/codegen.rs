//! Generated code: committed files are up to date (`--codegen --check`),
//! and they agree with PoB's own tables and functions under LuaJIT.

use std::path::{Path, PathBuf};

use mlua::{Function, Table, Value};

#[rustfmt::skip]
#[path = "../generated/global.rs"]
mod global;
#[rustfmt::skip]
#[path = "../generated/misc.rs"]
mod misc;

fn pob_src() -> Option<PathBuf> {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let candidates = [
        std::env::var_os("POB_SRC").map(PathBuf::from),
        Some(here.join("../../.PathOfBuilding/src")),
        Some(here.join("../../../../../.PathOfBuilding/src")),
    ];
    candidates
        .into_iter()
        .flatten()
        .find(|p| p.join("Modules/Data.lua").exists())
}

#[test]
fn generated_files_up_to_date() {
    let Some(src) = pob_src() else {
        eprintln!("skipping: PoB source not found (set POB_SRC)");
        return;
    };
    let stale = pob_dump::codegen::check_all(&src, &pob_dump::codegen::default_dir()).unwrap();
    assert!(
        stale.is_empty(),
        "run `cargo run -p pob-dump -- --codegen`: {stale:?}"
    );
}

/// The CLI: `--codegen --check` exits 0 on the committed files and fails
/// on a copy where one file differs or is missing; `--codegen` rewrites it.
#[test]
fn codegen_check_cli() {
    let Some(src) = pob_src() else {
        eprintln!("skipping: PoB source not found (set POB_SRC)");
        return;
    };
    let exe = env!("CARGO_BIN_EXE_pob-dump");
    let run = |dir: &Path, check: bool| {
        let mut c = std::process::Command::new(exe);
        c.arg("--pob")
            .arg(&src)
            .arg("--codegen")
            .arg("--generated")
            .arg(dir);
        if check {
            c.arg("--check");
        }
        c.output().unwrap()
    };
    let committed = pob_dump::codegen::default_dir();
    let ok = run(&committed, true);
    assert!(
        ok.status.success(),
        "{}",
        String::from_utf8_lossy(&ok.stderr)
    );

    let tmp = Path::new(env!("CARGO_TARGET_TMPDIR")).join("pob-dump-codegen-check");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    for f in pob_dump::codegen::GENERATED {
        std::fs::copy(committed.join(f), tmp.join(f)).unwrap();
    }
    assert!(run(&tmp, true).status.success());
    // one changed byte
    let p = tmp.join("misc.rs");
    let mut text = std::fs::read_to_string(&p).unwrap();
    text.push('\n');
    std::fs::write(&p, &text).unwrap();
    let bad = run(&tmp, true);
    assert!(!bad.status.success());
    let err = String::from_utf8_lossy(&bad.stderr);
    assert!(err.contains("misc.rs differs"), "{err}");
    // a missing file
    std::fs::remove_file(tmp.join("global.rs")).unwrap();
    assert!(!run(&tmp, true).status.success());
    // --codegen restores both, byte for byte
    assert!(run(&tmp, false).status.success());
    for f in pob_dump::codegen::GENERATED {
        assert_eq!(
            std::fs::read(tmp.join(f)).unwrap(),
            std::fs::read(committed.join(f)).unwrap(),
            "{f}"
        );
    }
    assert!(run(&tmp, true).status.success());
}

/// `pob_dump::Env` with Common + Global.lua loaded (as `--codegen-enums`).
fn global_env(src: &Path) -> pob_dump::Env {
    let env = pob_dump::Env::new(src).unwrap();
    env.lua
        .load(
            r#"
dofile("_SimpleGraphic.def.lua")
launch = { devMode = false }
LoadModule("Modules/Common")
LoadModule("Data/Global")
"#,
        )
        .exec()
        .unwrap();
    env
}

fn lua_entries(t: &Table) -> Vec<(String, f64)> {
    let mut v: Vec<(String, f64)> = t.pairs::<String, f64>().map(Result::unwrap).collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

fn sorted<T: Copy + Into<f64>>(e: &[(&str, T)]) -> Vec<(String, f64)> {
    let mut v: Vec<(String, f64)> = e
        .iter()
        .map(|(k, x)| ((*k).to_owned(), (*x).into()))
        .collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

#[test]
fn global_matches_lua() {
    use global::{KeywordFlag, ModFlag, SkillType, SkillTypeExpr, SkillTypeSet};
    let Some(src) = pob_src() else {
        eprintln!("skipping: PoB source not found (set POB_SRC)");
        return;
    };
    let env = global_env(&src);
    let g = env.lua.globals();
    let t = |n: &str| -> Table { g.get(n).unwrap() };
    assert_eq!(lua_entries(&t("ModFlag")), sorted(ModFlag::ENTRIES));
    assert_eq!(lua_entries(&t("KeywordFlag")), sorted(KeywordFlag::ENTRIES));
    assert_eq!(lua_entries(&t("SkillType")), sorted(SkillType::ENTRIES));
    assert_eq!(SkillType::ENTRIES.len(), 135);
    assert_eq!(
        (SkillType::Area, SkillType::Aura, SkillType::Pact),
        (8, 39, 135)
    );
    assert_eq!(SkillType::COUNT, 136);
    assert_eq!(SkillType::by_name("Aura"), Some(39));
    assert_eq!(SkillType::name(135), Some("Pact"));
    assert_eq!(SkillType::by_name("Nope"), None);
    assert_eq!(ModFlag::by_name("WeaponMask"), Some(0x2FFF_0000));
    assert_eq!(KeywordFlag::by_name("MatchAll"), Some(0x4000_0000));
    // ModFlag/KeywordFlag source order is Global.lua's
    assert_eq!(ModFlag::ENTRIES[0], ("Attack", 1));
    assert_eq!(KeywordFlag::ENTRIES.last().unwrap().0, "MatchAll");

    // MatchKeywordFlags against PoB's (Global.lua:189-214) on every pair of
    // a set of flag words covering single flags, unions and MatchAll.
    let lua_match: Function = g.get("MatchKeywordFlags").unwrap();
    let mut words: Vec<u32> = vec![0];
    for (_, f) in KeywordFlag::ENTRIES {
        words.push(*f);
    }
    let singles = words.clone();
    for (i, a) in singles.iter().enumerate() {
        for b in &singles[i..] {
            words.push(a | b);
            words.push(a | b | KeywordFlag::MatchAll);
        }
    }
    words.sort_unstable();
    words.dedup();
    let mut checked = 0;
    for &kw in words.iter().step_by(3) {
        for &m in &words {
            let want: bool = lua_match.call((kw, m)).unwrap();
            assert_eq!(
                global::MatchKeywordFlags(kw, m),
                want,
                "MatchKeywordFlags({kw:#x}, {m:#x})"
            );
            checked += 1;
        }
    }
    eprintln!("MatchKeywordFlags: {checked} pairs agree with Lua");

    // colorCodes, aliases included
    let cc = t("colorCodes");
    let n_lua = cc.pairs::<String, String>().count();
    assert_eq!(global::colorCodes.len(), n_lua);
    for (k, v) in global::colorCodes {
        assert_eq!(cc.get::<String>(*k).unwrap(), *v, "colorCodes.{k}");
    }

    // SkillTypeExpr / SkillTypeSet
    assert_eq!(
        SkillTypeExpr::from_skill_type(SkillType::AND),
        SkillTypeExpr::AND
    );
    assert_eq!(
        SkillTypeExpr::from_skill_type(SkillType::OR),
        SkillTypeExpr::OR
    );
    assert_eq!(
        SkillTypeExpr::from_skill_type(SkillType::NOT),
        SkillTypeExpr::NOT
    );
    assert_eq!(
        SkillTypeExpr::from_skill_type(SkillType::Spell),
        SkillTypeExpr::Type(2)
    );
    let mut s = SkillTypeSet::default();
    for t in [SkillType::Pact, SkillType::Attack, SkillType::Aura, 64, 63] {
        s.set(t, true);
    }
    assert!(s.has(SkillType::Pact) && s.has(64) && !s.has(SkillType::Spell));
    s.set(SkillType::Aura, false);
    assert_eq!(s.iter().collect::<Vec<_>>(), vec![1, 63, 64, 135]);
}

#[test]
fn misc_matches_lua() {
    let Some(src) = pob_src() else {
        eprintln!("skipping: PoB source not found (set POB_SRC)");
        return;
    };
    let env = pob_dump::Env::new(&src).unwrap();
    env.boot_pob().unwrap();
    let data: Table = env.lua.globals().get("data").unwrap();

    fn check<T>(
        data: &Table,
        name: &str,
        keys: &[&str],
        get: impl Fn(&T, &str) -> Option<f64>,
        pinned: T,
    ) where
        T: serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
    {
        let t: Table = data.get(name).unwrap();
        let mut obj = serde_json::Map::new();
        for pair in t.pairs::<String, Value>() {
            let (k, v) = pair.unwrap();
            let x = match v {
                Value::Number(n) => n,
                Value::Integer(i) => i as f64,
                other => panic!("data.{name}[{k}] = {other:?}"),
            };
            assert_eq!(get(&pinned, &k), Some(x), "data.{name}[{k:?}]");
            obj.insert(k, serde_json::Value::from(x));
        }
        assert_eq!(obj.len(), keys.len(), "data.{name}: key count");
        for k in keys {
            assert!(obj.contains_key(*k), "data.{name}: {k:?} not in Lua");
        }
        // deserialises from the plain JSON image of the Lua table ...
        let back: T = serde_json::from_value(serde_json::Value::Object(obj.clone())).unwrap();
        assert_eq!(back, pinned, "data.{name}");
        // ... and rejects keys it does not know
        obj.insert("not_a_key".into(), 1.into());
        assert!(serde_json::from_value::<T>(serde_json::Value::Object(obj)).is_err());
    }
    check(
        &data,
        "misc",
        misc::misc::KEYS,
        misc::misc::get,
        misc::misc::PINNED,
    );
    check(
        &data,
        "gameConstants",
        misc::gameConstants::KEYS,
        misc::gameConstants::get,
        misc::gameConstants::PINNED,
    );
    check(
        &data,
        "characterConstants",
        misc::characterConstants::KEYS,
        misc::characterConstants::get,
        misc::characterConstants::PINNED,
    );
    check(
        &data,
        "monsterConstants",
        misc::monsterConstants::KEYS,
        misc::monsterConstants::get,
        misc::monsterConstants::PINNED,
    );
    // verbatim keys through serde renames
    assert_eq!(
        misc::characterConstants::PINNED.get("base_maximum_all_resistances_%"),
        Some(75.0)
    );
    assert_eq!(
        misc::characterConstants::PINNED.base_maximum_all_resistances_pct,
        75.0
    );
    assert_eq!(misc::misc::PINNED.ServerTickRate, 1.0 / 0.033);
}
