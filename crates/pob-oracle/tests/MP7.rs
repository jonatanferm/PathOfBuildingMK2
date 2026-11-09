//! Unit MP7 acceptance (calc doc §11.1 row MP7, §9.4 L2 "Jewel funcs: each func on every node of
//! the 3_26 tree"): ModParser.lua 6151-6607 (`jewel_func_list.rs`) and the func bodies
//! (`pob_calc::modules::jewel_funcs::callJewelFunc`).
//!
//! * `list_shape`: the four jewel tables have PoB's entry counts and keys (source order); every
//!   jewelFuncList key lowercases one table key; every table value resolves (helper spec, known
//!   closure, capture closure, or `$list`).
//! * `mp7_oracle` (PoB booted under LuaJIT, JIT off): the func lines are every exact
//!   jewelFuncList key, every `JewelFunc` / `ExtraJewelFunc` line of `parse_oracle.jsonl`, and a
//!   sample line for every capture pattern. PoB parses each line (`modLib.parseMod`) and runs the
//!   func on every node of the 3_26 tree (sorted by id) plus synthetic tattoo and conquered
//!   overlays, exactly as CalcSetup does (`out` = the node's mods, one `data` per func, then the
//!   finish call `func(nil, out, data)`); `corpus/MP7/jewel_funcs.lua`. The Rust side parses the
//!   same line with MP0's driver over MP7's jewelFuncList (the two ExtraJewelFunc closures, whose
//!   specialModList entries are MP3-MP6's, are built per CCR-0034), rebuilds the same nodes from
//!   the dumped mods, runs `callJewelFunc` the same way and must give, for every call, the same
//!   error status, the same `out` list (untouched mods by identity, others by value) and the same
//!   `data` table, and the same finish `out` and `data`. Gate: 100% (calc doc §9.4 L2).
//!
//! Inputs: `data/dump`, the PoB checkout; missing inputs skip (fail with `POB_ORACLE_REQUIRE=1`
//! or `CI`). `MP7_VERBOSE=1` prints every mismatch; `MP7_FUNCS=n` runs the first n lines only.
#![allow(non_snake_case)]
#![allow(clippy::too_many_lines)]

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Once;

use pob_calc::modules::jewel_funcs::callJewelFunc;
use pob_calc::prelude::*;
use pob_gamedata::{JewelFuncKind, JewelFuncSpec};
use pob_modparser::jewel_func_list::{
    callCaptureClosure, jewelFuncList, jewelFuncListSource, jewelOtherFuncs, jewelSelfFuncs,
    jewelSelfUnallocFuncs, jewelThresholdFuncs,
};
use pob_oracle::headless::{Engine, Ret};
use pob_oracle::jit::JitPob;
use pob_oracle::oracle::{find_pob_src, find_up};
use pob_oracle::runner::OracleLock;
use serde_json::Value as J;

const SCRIPT: &str = include_str!("../corpus/MP7/jewel_funcs.lua");

fn skip(why: &str) {
    let required = std::env::var_os("POB_ORACLE_REQUIRE").is_some_and(|v| v == "1")
        || std::env::var_os("CI").is_some_and(|v| !v.is_empty() && v != "false" && v != "0");
    assert!(!required, "required input missing: {why}");
    eprintln!("SKIPPED: {why}");
}

fn dump_dir() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("POB_DUMP_DIR").map(PathBuf::from) {
        return p.join("data.bin").is_file().then_some(p);
    }
    find_up("data/dump/data.bin").map(|p| p.parent().unwrap().to_path_buf())
}

fn init_data() -> Option<PathBuf> {
    static INIT: Once = Once::new();
    let dir = dump_dir()?;
    INIT.call_once(|| {
        pob_gamedata::init_from_dir(&dir).unwrap_or_else(|e| panic!("data.bin: {e}"));
    });
    Some(dir)
}

/// `pob_lua::error::catch`: a LuaError panic as `Err` (other panics propagate).
fn catch<R>(f: impl FnOnce() -> R) -> Result<R, LuaError> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(r) => Ok(r),
        Err(p) => match p.downcast::<LuaError>() {
            Ok(e) => Err(*e),
            Err(other) => std::panic::resume_unwind(other),
        },
    }
}

fn quiet_lua_errors() {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if info.payload().downcast_ref::<LuaError>().is_none() {
            prev(info);
        }
    }));
}

/// `["key"] = …` keys at the top level of the table constructor in lines `a..=b`.
fn literal_keys(src: &[&str], a: usize, b: usize) -> Vec<String> {
    let mut v = Vec::new();
    for ln in a..=b {
        let s = src[ln - 1];
        if let Some(rest) = s.strip_prefix("\t[\"") {
            if let Some(end) = rest.find("\"] = ") {
                v.push(rest[..end].to_owned());
            }
        }
    }
    v
}

#[test]
fn list_shape() {
    if init_data().is_none() {
        return skip("data/dump not found");
    }
    let Some(src) = find_pob_src() else {
        return skip("PoB checkout not found");
    };
    let text = std::fs::read_to_string(src.join("Modules/ModParser.lua")).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    let tables: [(&str, &Vec<(&'static str, JewelFuncSpec)>, usize, usize); 4] = [
        ("jewelOtherFuncs", &jewelOtherFuncs, 6151, 6366),
        ("jewelSelfFuncs", &jewelSelfFuncs, 6378, 6408),
        ("jewelSelfUnallocFuncs", &jewelSelfUnallocFuncs, 6409, 6452),
        ("jewelThresholdFuncs", &jewelThresholdFuncs, 6480, 6549),
    ];
    let mut total = 0;
    for (name, t, a, b) in tables {
        let keys = literal_keys(&lines, a, b);
        let ours: Vec<String> = t.iter().map(|(k, _)| (*k).to_owned()).collect();
        assert_eq!(ours, keys, "{name}: keys differ from ModParser.lua {a}-{b}");
        eprintln!("{name}: {} entries", keys.len());
        total += keys.len();
        for (k, v) in t.iter() {
            let key = v.key.as_str();
            let known = key.starts_with('$')
                || key.starts_with("Modules/ModParser.lua:")
                || callCaptureClosure(v, &[Value::from("1")]).is_some();
            assert!(known, "{name}[{k}]: unknown value {key}");
        }
    }
    assert_eq!(jewelFuncList.len(), total, "jewelFuncList size");
    for (k, spec) in jewelFuncList.iter() {
        assert_eq!(*k, spec.key.as_str());
        assert!(jewelFuncListSource.contains_key(k), "{k}: no source");
        assert!(spec.r#type.is_some());
    }
    eprintln!("jewelFuncList: {} entries", jewelFuncList.len());
}

/// A sample line for every capture pattern (the parse oracle covers only some of them).
const CAPTURE_LINES: &[&str] = &[
    "Increases and Reductions to Energy Shield in Radius are Transformed to apply to Armour at 50% of their value",
    "Increases and Reductions to Life in Radius are Transformed to apply to Mana at 30% of their value",
    "Passives granting Lightning Resistance or all Elemental Resistances in Radius also grant Chance to Block Spell Damage at 35% of its value",
    "Passives granting Lightning Resistance or all Elemental Resistances in Radius also grant increased Maximum Energy Shield at 35% of its value",
    "Passives granting Lightning Resistance or all Elemental Resistances in Radius also grant Lightning Damage converted to Chaos Damage at 35% of its value",
    "Passives granting Cold Resistance or all Elemental Resistances in Radius also grant Chance to Dodge Attack Hits at 35% of its value",
    "Passives granting Cold Resistance or all Elemental Resistances in Radius also grant Chance to Suppress Spell Damage at 35% of its value",
    "Passives granting Cold Resistance or all Elemental Resistances in Radius also grant increased Maximum Mana at 35% of its value",
    "Passives granting Cold Resistance or all Elemental Resistances in Radius also grant Cold Damage converted to Chaos Damage at 35% of its value",
    "Passives granting Fire Resistance or all Elemental Resistances in Radius also grant Chance to Block Attack Damage at 35% of its value",
    "Passives granting Fire Resistance or all Elemental Resistances in Radius also grant Chance to Block at 35% of its value",
    "Passives granting Fire Resistance or all Elemental Resistances in Radius also grant increased Maximum Life at 35% of its value",
    "Passives granting Fire Resistance or all Elemental Resistances in Radius also grant Fire Damage converted to Chaos Damage at 35% of its value",
    "15% increased Effect of non-Keystone Passive Skills in Radius",
    "50% increased effect of Tattoos in Radius",
    "Passive Skills in Radius also grant: Traps and Mines deal 2 to 4 added Physical Damage",
    "Passive Skills in Radius also grant: 1% increased Unarmed Attack Speed with Melee Skills",
    "Passive Skills in Radius also grant 5% increased Global Critical Strike Chance",
    "Passive Skills in Radius also grant +5 to Maximum Life",
    "Passive Skills in Radius also grant +5 to Maximum Mana",
    "Passive Skills in Radius also grant 3% increased Energy Shield",
    "Passive Skills in Radius also grant 3% increased Armour",
    "Passive Skills in Radius also grant 3% increased Evasion Rating",
    "Passive Skills in Radius also grant +2 to all Attributes",
    "Passive Skills in Radius also grant +2% to Chaos Resistance",
    "Passive Skills in Radius also grant 4% increased Fire Damage",
    "Notable Passive Skills in Radius are Transformed to instead grant: 10% increased Mana Cost of Skills and 20% increased Spell Damage",
    "Notable Passive Skills in Radius are Transformed to instead grant: Minions take 20% increased Damage",
    "Notable Passive Skills in Radius are Transformed to instead grant: Minions have 20% reduced Movement Speed",
    "With at least 40 Dexterity in Radius, Dual Strike has 20% increased Attack Speed while wielding a Claw",
    "With at least 40 Dexterity in Radius, Dual Strike has +20% to Critical Strike Multiplier while wielding a Dagger",
    "With at least 40 Dexterity in Radius, Dual Strike has 50% increased Accuracy Rating while wielding a Sword",
    "With at least 40 Intelligence in Radius, Fireball has +25% chance to inflict scorch",
    "With at least 40 Intelligence in Radius, 10% of Damage taken Recouped as Mana if you've Warcried Recently",
    "With at least 40 Intelligence in Radius, Fireball Projectiles gain Radius as they travel farther, up to +4 Radius",
    "With at least 40 Intelligence in Radius, Projectiles gain radius as they travel farther, up to a maximum of +0.4 metres to radius",
    "Non-Unique Jewels cause Increases and Reductions to other Damage Types in a Large Radius to be Transformed to apply to Cold Damage",
    "Non-Unique Jewels cause Small and Notable Passive Skills in a Medium Radius to also grant +4 to Intelligence",
];

/// The two specialModList ExtraJewelFunc entries (ModParser.lua:2621/2629) as CCR-0034 encodes
/// them (their lists are MP3-MP6's).
fn extra_jewel_func(line: &str) -> Option<JewelFuncSpec> {
    let lower = pob_lua::string::lower(line);
    let mk = |key: &str, args: Vec<Value>, radius: &str| JewelFuncSpec {
        key: Name::from(key),
        kind: JewelFuncKind::Other,
        args: args.into_iter().collect(),
        r#type: Some(n!("Other")),
        radius: Some(Name::from(pob_lua::string::firstToUpper(radius).as_str())),
    };
    let p1 = "^non%-unique jewels cause increases and reductions to other damage types in a (%a+) radius to be transformed to apply to (%a+) damage$";
    if let Some(c) = pob_lua::lmatch(&lower, p1, None) {
        let s: Vec<String> = c.iter().map(|x| x.to_str().into_owned()).collect();
        return Some(mk(
            "Modules/ModParser.lua:2622-2627",
            vec![Value::from(s[1].as_str())],
            &s[0],
        ));
    }
    let p2 = "^non%-unique jewels cause small and notable passive skills in a (%a+) radius to also grant %+(%d+) to (%a+)$";
    if let Some(c) = pob_lua::lmatch(&lower, p2, None) {
        let s: Vec<String> = c.iter().map(|x| x.to_str().into_owned()).collect();
        return Some(mk(
            "Modules/ModParser.lua:2630-2634",
            vec![Value::from(s[1].as_str()), Value::from(s[2].as_str())],
            &s[0],
        ));
    }
    None
}

fn func_lines(dir: &std::path::Path) -> Vec<String> {
    let mut set: BTreeSet<String> = BTreeSet::new();
    for t in [
        &*jewelOtherFuncs,
        &*jewelSelfFuncs,
        &*jewelSelfUnallocFuncs,
        &*jewelThresholdFuncs,
    ] {
        for (k, _) in t.iter() {
            if !k.contains('(') {
                set.insert((*k).to_owned());
            }
        }
    }
    for l in CAPTURE_LINES {
        set.insert((*l).to_owned());
    }
    match std::fs::read_to_string(dir.join("parse_oracle.jsonl")) {
        Ok(text) => {
            let mut n = 0;
            for l in text.lines() {
                if !l.contains("JewelFunc\"") {
                    continue;
                }
                let r: J = serde_json::from_str(l).unwrap();
                let is_jewel = r["mods"].as_array().is_some_and(|m| {
                    m.iter().any(|x| {
                        x["name"]
                            .as_str()
                            .is_some_and(|s| s == "JewelFunc" || s == "ExtraJewelFunc")
                    })
                });
                if is_jewel {
                    set.insert(r["line"].as_str().unwrap().to_owned());
                    n += 1;
                }
            }
            eprintln!("parse oracle: {n} jewel func lines");
        }
        Err(_) => eprintln!("parse_oracle.jsonl not found: table keys and samples only"),
    }
    set.into_iter().collect()
}

/// A mod entry `{"mod": {…}, "marks": {…}}`.
fn decode_mod(e: &J) -> Mod {
    let mut m: Mod =
        serde_json::from_value(e["mod"].clone()).unwrap_or_else(|x| panic!("mod {e}: {x}"));
    if let Some(marks) = e["marks"].as_object() {
        if !marks.is_empty() {
            let mut ex = m.extra.take().map(|b| *b).unwrap_or_default();
            for (k, v) in marks {
                let v: Value = serde_json::from_value(v.clone()).unwrap();
                ex.set(Name::from(k.as_str()), v);
            }
            m.extra = Some(Box::new(ex));
        }
    }
    m
}

fn jnum(v: &J) -> Option<f64> {
    match v {
        J::Number(n) => n.as_f64(),
        J::Object(o) => match o.get("$float").and_then(J::as_str) {
            Some("-0") => Some(-0.0),
            Some("nan") => Some(f64::NAN),
            Some("inf") => Some(f64::INFINITY),
            Some("-inf") => Some(f64::NEG_INFINITY),
            _ => None,
        },
        _ => None,
    }
}

fn key_text(k: Key) -> String {
    match k {
        Key::S(s) => s.as_str().to_owned(),
        Key::I(i) => i.to_string(),
        Key::B(b) => b.to_string(),
    }
}

/// The Lua `data` encoding (corpus script `encData`) against the Rust table.
fn cmp_data(lua: &J, rust: &Val, path: &str) -> Result<(), String> {
    match (lua, rust) {
        (J::Null, Val::Nil) => Ok(()),
        (J::Bool(a), Val::Bool(b)) if a == b => Ok(()),
        (J::String(a), Val::Str(b)) if a.as_str() == &**b => Ok(()),
        (J::Object(o), Val::Obj(_)) => {
            let Some(l) = rust.obj::<ModStore>() else {
                return Err(format!("{path}: userdata is not a ModList"));
            };
            if let Some(n) = o.get("$ModList#") {
                return if n.as_u64() == Some(l.len() as u64) {
                    Ok(())
                } else {
                    Err(format!("{path}: ModList length {n} vs {}", l.len()))
                };
            }
            let Some(J::Array(mods)) = o.get("$ModList") else {
                return Err(format!("{path}: Lua {lua} vs a ModList"));
            };
            let ours = l.ipairs();
            if mods.len() != ours.len() {
                return Err(format!(
                    "{path}: ModList length {} vs {}",
                    mods.len(),
                    ours.len()
                ));
            }
            for (i, (a, b)) in mods.iter().zip(ours.iter()).enumerate() {
                let a = decode_mod(a);
                if a != **b {
                    return Err(format!("{path}[{}]: {a:?} vs {b:?}", i + 1));
                }
            }
            Ok(())
        }
        (J::Object(o), Val::Table(t)) if !o.contains_key("$float") => {
            let mut ours: Vec<(String, Val)> = t.pairs().map(|(k, v)| (key_text(k), v)).collect();
            ours.sort_by(|a, b| a.0.cmp(&b.0));
            let lk: Vec<&String> = o.keys().collect();
            let rk: Vec<&String> = ours.iter().map(|(k, _)| k).collect();
            let mut lks = lk.clone();
            lks.sort();
            if lks != rk {
                return Err(format!("{path}: keys {lks:?} vs {rk:?}"));
            }
            for (k, v) in &ours {
                cmp_data(&o[k], v, &format!("{path}.{k}"))?;
            }
            Ok(())
        }
        (l, Val::Num(b)) => match jnum(l) {
            // ±0 is a host artefact on a dual-number oracle (CCR-0002); == ignores the sign
            Some(a) if a == *b || (a.is_nan() && b.is_nan()) => Ok(()),
            _ => Err(format!("{path}: {l} vs {b}")),
        },
        (l, r) => Err(format!("{path}: Lua {l} vs Rust {r:?}")),
    }
}

struct RNode {
    node: NodeRef,
    keystone: bool,
    has_list: bool,
}

fn build_nodes(doc: &J) -> Vec<RNode> {
    doc["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| {
            let modList = ModStore::ModList(None);
            let has_list = !n["mods"].is_null();
            if let Some(ms) = n["mods"].as_array() {
                for m in ms {
                    modList.AddMod(Arc::new(decode_mod(m)));
                }
            }
            let ty = Name::from(n["type"].as_str().unwrap());
            let ks = (!n["ks"].is_null()).then(|| Arc::new(decode_mod(&n["ks"])));
            let conq = n["conq"].as_bool().unwrap();
            let node = Node {
                id: n["id"].as_f64().unwrap(),
                ..Default::default()
            };
            node.r#type.set(ty);
            node.isTattoo.set(n["tattoo"].as_bool().unwrap());
            node.modList.set(modList);
            node.keystoneMod.set(ks);
            if conq {
                let c = Table::new();
                c.set(n!("id"), 1.0);
                node.conqueredBy.set(Some(c));
            }
            RNode {
                keystone: ty == n!("Keystone"),
                node: Rc::new(node),
                has_list,
            }
        })
        .collect()
}

/// The func of a line, as the Rust side gets it (MP0's driver; CCR-0034 for ExtraJewelFunc).
fn rust_func(line: &str) -> Result<(Name, Option<Name>, JewelFuncSpec), String> {
    if let Some(s) = extra_jewel_func(line) {
        return Ok((n!("ExtraJewelFunc"), Some(n!("Other")), s));
    }
    let (mods, _extra) =
        catch(|| pob_modparser::parseModNative(line)).map_err(|e| format!("parse error: {e}"))?;
    let Some(m) = mods.and_then(|m| m.into_iter().next()) else {
        return Err("no mods".to_owned());
    };
    let Value::JewelFunc(f) = m.value.get(n!("func")) else {
        return Err(format!("not a jewel func: {:?}", m.value));
    };
    let ty = m.value.get(n!("type")).as_str().map(Name::from);
    Ok((m.name, ty, (**f).clone()))
}

#[derive(Default)]
struct Tally {
    funcs: usize,
    funcs_ok: usize,
    calls: usize,
    calls_ok: usize,
    errors_both: usize,
    data_checks: usize,
    data_ok: usize,
    changed: usize,
    finish: usize,
    finish_ok: usize,
    finish_nonempty: usize,
    mismatches: Vec<String>,
}

#[test]
fn mp7_oracle() {
    let Some(dir) = init_data() else {
        return skip("data/dump not found");
    };
    let Some(src) = find_pob_src() else {
        return skip("PoB checkout not found");
    };
    quiet_lua_errors();
    let verbose = std::env::var_os("MP7_VERBOSE").is_some();
    let mut lines = func_lines(&dir);
    if let Some(n) = std::env::var("MP7_FUNCS")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
    {
        lines.truncate(n);
    }
    eprintln!("{} func lines", lines.len());
    let mut lua_lines = String::from("__mp7_lines = {\n");
    for l in &lines {
        assert!(!l.contains("]==]"));
        lua_lines.push_str(&format!("[==[{l}]==],\n"));
    }
    lua_lines.push_str("}\n");
    let json = {
        let _lock = OracleLock::acquire().expect("oracle lock");
        let t0 = std::time::Instant::now();
        let mut pob = JitPob::boot_oracle(&src, None).unwrap_or_else(|e| panic!("boot: {e}"));
        pob.run(&lua_lines).unwrap_or_else(|e| panic!("lines: {e}"));
        let r = match pob
            .run(SCRIPT)
            .unwrap_or_else(|e| panic!("script: {e}"))
            .first()
        {
            Some(Ret::Str(s)) => s.clone(),
            other => panic!("script returned {other:?}"),
        };
        eprintln!(
            "oracle: {:.1}s, {} bytes",
            t0.elapsed().as_secs_f64(),
            r.len()
        );
        r
    };
    let doc: J = serde_json::from_str(&json).unwrap();
    let nodes = build_nodes(&doc);
    let synth = nodes
        .iter()
        .filter(|n| n.node.isTattoo.get() || n.node.conqueredBy.get().is_some())
        .count();
    let no_list = nodes.iter().filter(|n| !n.has_list).count();
    eprintln!(
        "{} nodes ({} synthetic overlays, {} without a modList: skipped on both sides)",
        nodes.len(),
        synth,
        no_list
    );
    let mut t = Tally::default();
    let mut covered: BTreeSet<String> = BTreeSet::new();
    let funcs = doc["funcs"].as_array().unwrap();
    assert_eq!(funcs.len(), lines.len());
    for (line, rec) in lines.iter().zip(funcs) {
        t.funcs += 1;
        let before_mm = t.mismatches.len();
        let mism = |t: &mut Tally, s: String| t.mismatches.push(format!("{line}: {s}"));
        if rec.get("parseError").is_some() || rec.get("noFunc").is_some() {
            mism(&mut t, format!("PoB gives no func: {rec}"));
            continue;
        }
        let (name, ty, spec) = match rust_func(line) {
            Ok(x) => x,
            Err(e) => {
                mism(&mut t, format!("Rust: {e}"));
                continue;
            }
        };
        if !spec.key.as_str().starts_with('$') && !spec.key.as_str().starts_with("Modules/") {
            covered.insert(spec.key.as_str().to_owned());
        }
        if name.as_str() != rec["name"].as_str().unwrap()
            || ty.map(|n| n.as_str().to_owned()) != rec["type"].as_str().map(str::to_owned)
        {
            mism(
                &mut t,
                format!(
                    "mod {name} type {ty:?} vs PoB {} {}",
                    rec["name"], rec["type"]
                ),
            );
        }
        let calls = rec["calls"].as_array().unwrap();
        let datas = rec["datas"].as_array().unwrap();
        let (mut ci, mut di) = (0usize, 0usize);
        let data = Table::new();
        data.set(n!("modSource"), "Tree:61419");
        let mut lua_data = serde_json::json!({ "modSource": "Tree:61419" });
        for (i0, rn) in nodes.iter().enumerate() {
            if !rn.has_list {
                continue;
            }
            let i = (i0 + 1) as u64;
            let out = ModStore::ModList(None);
            if rn.keystone {
                if let Some(ks) = rn.node.keystoneMod.get() {
                    out.AddMod(ks);
                }
            } else {
                out.AddList(&rn.node.modList.get().ipairs());
            }
            let before = out.ipairs();
            let r = catch(|| callJewelFunc(&spec, Some(&rn.node), &out, &data));
            let after = out.ipairs();
            let changed = r.is_err()
                || after.len() != before.len()
                || after
                    .iter()
                    .zip(before.iter())
                    .any(|(a, b)| !Arc::ptr_eq(a, b));
            let lrec = calls.get(ci).filter(|c| c["i"].as_u64() == Some(i));
            if lrec.is_some() {
                ci += 1;
            }
            t.calls += 1;
            match (lrec, changed) {
                (None, false) => t.calls_ok += 1,
                (None, true) => mism(
                    &mut t,
                    format!(
                        "node #{i} id {}: Rust changed out ({r:?}), PoB did not",
                        rn.node.id
                    ),
                ),
                (Some(c), false) => mism(
                    &mut t,
                    format!(
                        "node #{i} id {}: PoB changed out ({}), Rust did not",
                        rn.node.id, c["err"]
                    ),
                ),
                (Some(c), true) => {
                    let lerr = !c["err"].is_null();
                    if lerr != r.is_err() {
                        mism(
                            &mut t,
                            format!("node #{i}: error PoB {} vs Rust {r:?}", c["err"]),
                        );
                    } else if lerr {
                        t.errors_both += 1;
                        t.calls_ok += 1;
                    } else {
                        let lo = c["out"].as_array().unwrap();
                        let mut ok = lo.len() == after.len();
                        let mut why = format!("len {} vs {}", lo.len(), after.len());
                        if ok {
                            for (k, (l, rm)) in lo.iter().zip(after.iter()).enumerate() {
                                if let Some(j) = l.get("$same").and_then(J::as_u64) {
                                    if !Arc::ptr_eq(rm, &before[j as usize - 1]) {
                                        ok = false;
                                        why = format!(
                                            "out[{}]: PoB keeps input mod {j}, Rust has {rm:?}",
                                            k + 1
                                        );
                                        break;
                                    }
                                } else {
                                    let lm = decode_mod(l);
                                    if lm != **rm {
                                        ok = false;
                                        why = format!("out[{}]: PoB {lm:?} vs Rust {rm:?}", k + 1);
                                        break;
                                    }
                                }
                            }
                        }
                        if ok {
                            t.calls_ok += 1;
                        } else {
                            mism(&mut t, format!("node #{i} id {}: {why}", rn.node.id));
                        }
                    }
                }
            }
            if let Some(d) = datas.get(di).filter(|d| d["i"].as_u64() == Some(i)) {
                di += 1;
                lua_data = d["d"].clone();
            }
            t.data_checks += 1;
            match cmp_data(&lua_data, &Val::Table(data.clone()), "data") {
                Ok(()) => t.data_ok += 1,
                Err(e) => mism(&mut t, format!("node #{i} id {}: {e}", rn.node.id)),
            }
        }
        if ci != calls.len() || di != datas.len() {
            mism(
                &mut t,
                format!(
                    "unconsumed PoB records: calls {ci}/{}, datas {di}/{}",
                    calls.len(),
                    datas.len()
                ),
            );
        }
        t.changed += calls.len();
        // the finish calls (the second with another source: getThreshold's shared `value.mod`)
        for (fi, fkey) in ["finish", "finish2"].into_iter().enumerate() {
            if fi == 1 {
                data.set(n!("modSource"), "Tree:2");
            }
            let out = ModStore::ModList(None);
            let r = catch(|| callJewelFunc(&spec, None, &out, &data));
            let f = &rec[fkey];
            t.finish += 1;
            let mut fok = true;
            if f["err"].is_null() != r.is_ok() {
                fok = false;
                mism(
                    &mut t,
                    format!("{fkey}: error PoB {} vs Rust {r:?}", f["err"]),
                );
            } else {
                if r.is_err() {
                    t.errors_both += 1;
                }
                let lo = f["out"].as_array().unwrap();
                let ro = out.ipairs();
                if !ro.is_empty() {
                    t.finish_nonempty += 1;
                }
                if lo.len() != ro.len() {
                    fok = false;
                    mism(
                        &mut t,
                        format!("{fkey}: out len {} vs {}", lo.len(), ro.len()),
                    );
                } else {
                    for (k, (l, rm)) in lo.iter().zip(ro.iter()).enumerate() {
                        let lm = decode_mod(l);
                        if lm != **rm {
                            fok = false;
                            mism(
                                &mut t,
                                format!("{fkey}: out[{}]: PoB {lm:?} vs Rust {rm:?}", k + 1),
                            );
                            break;
                        }
                    }
                }
                if let Err(e) = cmp_data(&f["data"], &Val::Table(data.clone()), fkey) {
                    fok = false;
                    mism(&mut t, e);
                }
            }
            if fok {
                t.finish_ok += 1;
            }
        }
        if t.mismatches.len() == before_mm {
            t.funcs_ok += 1;
        }
    }
    let uncovered: Vec<&str> = jewelFuncList
        .iter()
        .map(|(k, _)| *k)
        .filter(|k| !covered.contains(*k))
        .collect();
    eprintln!(
        "MP7: funcs {}/{} identical; calls {}/{} ({} changed out in PoB; both raised: {}); data {}/{}; finish calls {}/{} ({} non-empty out); jewelFuncList keys covered {}/{}",
        t.funcs_ok,
        t.funcs,
        t.calls_ok,
        t.calls,
        t.changed,
        t.errors_both,
        t.data_ok,
        t.data_checks,
        t.finish_ok,
        t.finish,
        t.finish_nonempty,
        jewelFuncList.len() - uncovered.len(),
        jewelFuncList.len()
    );
    let shown = if verbose { t.mismatches.len() } else { 20 };
    for m in t.mismatches.iter().take(shown) {
        eprintln!("  MISMATCH {m}");
    }
    if std::env::var("MP7_FUNCS").is_err() {
        assert!(
            uncovered.is_empty(),
            "jewelFuncList keys no line reaches: {uncovered:?}"
        );
    }
    assert!(t.mismatches.is_empty(), "{} mismatches", t.mismatches.len());
}
