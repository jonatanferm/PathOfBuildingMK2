//! Oracle: every line in PoB's `Data/ModCache.lua` must parse to exactly the
//! mods PoB itself cached for it. Self-skips without a PoB checkout.

use std::path::Path;

use pob_calc::lua::{Interp, Val};
use pob_calc::modifier::{Mod, Value};

/// ModCache.lua prints floats with 14 significant digits.
fn close(a: f64, b: f64) -> bool {
    a == b || (a - b).abs() <= 1e-9 * a.abs().max(b.abs())
}

fn value_close(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Num(x), Value::Num(y)) => close(*x, *y),
        (Value::Mod(x), Value::Mod(y)) => mod_close(x, y),
        (Value::Table(x), Value::Table(y)) => {
            x.array.len() == y.array.len()
                && x.array.iter().zip(&y.array).all(|(p, q)| value_close(p, q))
                && x.fields.len() == y.fields.len()
                && x.fields
                    .iter()
                    .all(|(k, v)| y.get(k).is_some_and(|w| value_close(v, w)))
        }
        _ => a == b,
    }
}

fn mod_close(a: &Mod, b: &Mod) -> bool {
    let strip = |m: &Mod| {
        let mut m = m.clone();
        m.value = Value::Nil;
        for t in &mut m.tags {
            for f in [
                &mut t.limit,
                &mut t.percent,
                &mut t.div,
                &mut t.threshold,
                &mut t.base,
            ] {
                *f = f.map(|v| (v * 1e9).round() / 1e9);
            }
        }
        m
    };
    value_close(&a.value, &b.value) && strip(a) == strip(b)
}

fn mods_close(a: &Option<Vec<Mod>>, b: &Option<Vec<Mod>>) -> bool {
    match (a, b) {
        (Some(x), Some(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| mod_close(p, q)),
        (None, None) => true,
        _ => false,
    }
}
use pob_calc::mod_parser::{lua_to_mod, ModParser};

#[test]
fn modcache_oracle() {
    let pob = Path::new("../../.PathOfBuilding/src");
    let env = Path::new("../../data/lua/modparser_env.lua");
    if !pob.exists() || !env.exists() {
        return;
    }
    let parser = ModParser::from_paths(pob, env).unwrap_or_else(|e| panic!("{e}"));
    let mut it = Interp::new();
    let src = std::fs::read_to_string(pob.join("Data/ModCache.lua")).unwrap();
    let t = std::time::Instant::now();
    let cache = it
        .exec_chunk(&src, "ModCache.lua", Vec::new())
        .unwrap_or_else(|e| panic!("{e}"));
    eprintln!("ModCache load: {:?}", t.elapsed());
    let cache = cache[0].as_table().unwrap().clone();
    let entries = cache.pairs();
    let limit: usize = std::env::var("ORACLE_LIMIT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(usize::MAX);
    let (mut ok, mut bad, mut errs) = (0usize, 0usize, 0usize);
    let mut failures: Vec<String> = Vec::new();
    let t = std::time::Instant::now();
    for (k, v) in entries.iter().take(limit) {
        let Val::Str(line) = k else { continue };
        let expected = v.as_table().unwrap();
        let exp_mods = match expected.get_int(1) {
            Val::Table(t) => Some(
                t.array()
                    .iter()
                    .map(|m| lua_to_mod(m).unwrap())
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        };
        let exp_extra = expected.get_int(2).as_str().map(str::to_owned);
        match parser.parse(line.as_str()) {
            Ok(got) => {
                if mods_close(&got.mods, &exp_mods) && got.extra == exp_extra {
                    ok += 1;
                } else {
                    bad += 1;
                    if failures.len() < 40 {
                        let show = |m: &Option<Vec<pob_calc::Mod>>| {
                            m.as_ref().map_or_else(
                                || "nil".to_owned(),
                                |v| {
                                    v.iter()
                                        .map(ToString::to_string)
                                        .collect::<Vec<_>>()
                                        .join(" ; ")
                                },
                            )
                        };
                        failures.push(format!(
                            "{line:?}\n   expected {} extra={exp_extra:?}\n   got      {} extra={:?}",
                            show(&exp_mods),
                            show(&got.mods),
                            got.extra
                        ));
                    }
                }
            }
            Err(e) => {
                errs += 1;
                if failures.len() < 40 {
                    failures.push(format!("{line:?}: ERROR {e}"));
                }
            }
        }
    }
    eprintln!("parse time: {:?}", t.elapsed());
    for f in &failures {
        eprintln!("{f}");
    }
    let total = ok + bad + errs;
    eprintln!("ModCache oracle: {ok}/{total} identical, {bad} differ, {errs} errors");
    assert!(ok as f64 >= total as f64 * 0.999, "parity below 99.9%");
}
