//! Unit PF-F acceptance (calc doc §11.1 row PF-F, §9.4 L4): CalcPerform.lua 2997-3496
//! (`calc_perform::party_curses::seg`).
//!
//! **L4 `perform:2997` → `perform:3497`**: inject PoB's `perform:2997` dump, run the Rust
//! segment, diff the state against PoB's `perform:3497` dump (env, GlobalCache, the perform
//! locals; §9.2 tolerance and the build's nondet record).
//!
//! The segment calls functions other units own. Those still `unported!` are replayed from PoB
//! (calc doc §9.2 "Replay", `pob_oracle::replay`): the unit's stage file
//! `corpus/PF-F/stages.toml` adds line probes around the two call groups whose effects only
//! the callee produces:
//!
//! * `perform:3118` → `perform:3125`: `mergeFlasks(env.flasks, true, …)` (PF-C, L3121);
//! * `perform:3484` → `perform:3497`: `calcs.defenceForConditionals` (DF-A) and
//!   `doActorCharges` / `doActorMisc` (PF-B), the last statements of the segment.
//!
//! A delta is replayed after the segment ran, and only when every callee of its group was a
//! stub in this run (a ported callee already did its part; a partly ported group is reported
//! as blocked). Nothing the segment reads after L3121 depends on what mergeFlasks writes in
//! the corpus (a cascade would show up as a mismatch).
//!
//! Other stubbed callees (`mergeBuff`, `determineCursePriority`, `addMinionModifiers`,
//! `calcs.mergeSkillInstanceMods`, `calcs.resistances`, `doActorLifeMana*`) have no probe; a
//! build that reaches one of them and then differs is counted **blocked**, not failed.
//!
//! Verdict per build: `pass` (no mismatch), `blocked` (mismatches and a non-replayed stub ran),
//! `fail` (mismatches with every callee ported or replayed). Gate: no `fail`, at least one
//! `pass`.
//!
//! Inputs: `data/dump`; `.parity/builds/*.xml` (all57) and `corpus/PF-F/*.xml` (party micro
//! builds); dumps from
//! `scripts/port/oracle.sh run --build X.xml --stages crates/pob-oracle/corpus/PF-F/stages.toml
//!  --stage perform:2997@main,perform:3118@main,perform:3125@main,perform:3484@main,perform:3497@main`.
//! Knobs: `PFF_BUILDS=stem1,stem2`, `PFF_VERBOSE=1` (print mismatches). A missing input skips
//! with a message; with `POB_ORACLE_REQUIRE=1` (or `CI`) it fails instead.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use pob_oracle::diff::{diff_trees, DiffOptions, Mismatch, MismatchKind};
use pob_oracle::nondet::NondetRecord;
use pob_oracle::oracle::find_parity_dir;
use pob_oracle::replay::{Call, Replay};
use pob_oracle::schema::{resolve_ids, StateDump};
use pob_oracle::testkit;
use serde_json::Value as J;

fn skip(why: &str) {
    let required = std::env::var_os("POB_ORACLE_REQUIRE").is_some_and(|v| v == "1")
        || std::env::var_os("CI").is_some_and(|v| !v.is_empty() && v != "false" && v != "0");
    assert!(!required, "required input missing: {why}");
    eprintln!("SKIPPED: {why}");
}

fn stem(p: &Path) -> &str {
    p.file_stem().and_then(|s| s.to_str()).unwrap_or_default()
}

fn xmls(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|r| {
            r.filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "xml"))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

/// all57 plus the unit's micro builds.
fn corpus(parity: &Path) -> Vec<PathBuf> {
    let mut v = xmls(&parity.join("builds"));
    v.extend(xmls(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("corpus/PF-F"),
    ));
    if let Ok(list) = std::env::var("PFF_BUILDS") {
        let want: Vec<&str> = list.split(',').map(str::trim).collect();
        v.retain(|p| want.contains(&stem(p)));
    }
    v
}

/// The stub sites of the callees replayed from PoB, by delta.
const FLASK_GROUP: &[&str] = &["Modules/CalcPerform.lua:1691-1861"];
const TAIL_GROUP: &[&str] = &[
    "Modules/CalcDefence.lua:481-508",
    "Modules/CalcPerform.lua:980-1139",
    "Modules/CalcPerform.lua:636-977",
];

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
enum Verdict {
    Pass,
    Blocked,
    Fail,
    Missing,
}

struct Outcome {
    verdict: Verdict,
    mismatches: Vec<Mismatch>,
    /// mismatches explained by a recorded contract gap (CCR-0030 row 7, `notBuff`)
    known: Vec<String>,
    /// stub sites the run hit that no delta replaced
    blockers: Vec<String>,
    replayed: Vec<&'static str>,
    note: String,
}

fn load(dirs: &[PathBuf], hit: &str) -> Option<StateDump> {
    testkit::find_dump(dirs, hit).and_then(|p| StateDump::load(&p).ok())
}

fn run_build(xml: &Path) -> Outcome {
    let stem = stem(xml).to_owned();
    let dirs = testkit::dump_dirs(&stem, Some(xml));
    let hits = [
        "perform:2997",
        "perform:3118",
        "perform:3125",
        "perform:3484",
        "perform:3497",
    ];
    let dumps: Vec<Option<StateDump>> = hits.iter().map(|h| load(&dirs, h)).collect();
    if dumps.iter().any(Option::is_none) {
        return Outcome {
            verdict: Verdict::Missing,
            mismatches: Vec::new(),
            known: Vec::new(),
            blockers: Vec::new(),
            replayed: Vec::new(),
            note: format!(
                "missing dumps {:?}",
                hits.iter()
                    .zip(&dumps)
                    .filter(|(_, d)| d.is_none())
                    .map(|(h, _)| *h)
                    .collect::<Vec<_>>()
            ),
        };
    }
    let d: Vec<StateDump> = dumps.into_iter().map(Option::unwrap).collect();
    let (din, d3118, d3125, d3484, dout) = (&d[0], &d[1], &d[2], &d[3], &d[4]);
    // one env: every probe must come from the same perform run
    let envs: Vec<&Option<String>> = d.iter().map(|x| &x.env_id).collect();
    let same_n = d.iter().all(|x| x.n == din.n);
    if envs.iter().any(|e| *e != envs[0]) || !same_n {
        let note = format!(
            "dumps of different perform runs: {:?}",
            d.iter().map(|x| (&x.hit, &x.env_id)).collect::<Vec<_>>()
        );
        // the line probes are numbered per probe id: the hit numbers differ when an
        // earlier probe fires on an env the later ones never reach. Same env is required.
        if envs.iter().any(|e| *e != envs[0]) {
            return Outcome {
                verdict: Verdict::Missing,
                mismatches: Vec::new(),
                known: Vec::new(),
                blockers: Vec::new(),
                replayed: Vec::new(),
                note,
            };
        }
    }
    let build = testkit::empty_build();
    let inj = match pob_oracle::inject::inject(din, &build) {
        Ok(i) => i,
        Err(e) => {
            return Outcome {
                verdict: Verdict::Fail,
                mismatches: Vec::new(),
                known: Vec::new(),
                blockers: Vec::new(),
                replayed: Vec::new(),
                note: format!("inject: {e}"),
            }
        }
    };
    let env = inj.env.clone().expect("perform:2997 dump has an env");
    let locals: Vec<(String, pob_calc::Val)> = inj.locals.into_iter().collect();
    pob_calc::stub::stub_reset();
    let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        pob_calc::probe::run_segment("perform:2997", &locals)
    }));
    let stubs: Vec<String> = pob_calc::stub::stub_report()
        .into_iter()
        .map(|(s, _)| s.to_owned())
        .collect();
    let after = match run {
        Ok(Ok((_flow, after))) => after,
        Ok(Err(e)) => {
            return Outcome {
                verdict: Verdict::Fail,
                mismatches: Vec::new(),
                known: Vec::new(),
                blockers: stubs,
                replayed: Vec::new(),
                note: format!("run: {e}"),
            }
        }
        Err(p) => {
            let msg = p
                .downcast_ref::<pob_lua::LuaError>()
                .map(|e| e.0.clone())
                .or_else(|| p.downcast_ref::<String>().cloned())
                .or_else(|| p.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                .unwrap_or_else(|| "panic".to_owned());
            return Outcome {
                verdict: Verdict::Fail,
                mismatches: Vec::new(),
                known: Vec::new(),
                blockers: stubs,
                replayed: Vec::new(),
                note: format!("run panicked: {msg}"),
            };
        }
    };
    // L3221 `env.curseSlots = curseSlots` always runs. env.curseSlots was nil at
    // perform:2997, so the injection marked it absent, and the exporter leaves an absent
    // field out while it holds the default (empty) table: unmark it, as the Lua key is set now.
    pob_calc::probe::unmark_absent(pob_calc::probe::rc_ptr(&env), "curseSlots");
    // replay the stubbed callee groups
    let mut blockers: Vec<String> = stubs.clone();
    let mut replayed = Vec::new();
    let mut notes = Vec::new();
    for (name, group, a, b) in [
        ("mergeFlasks", FLASK_GROUP, d3118, d3125),
        ("defenceForConditionals+doActorCharges+doActorMisc", TAIL_GROUP, d3484, dout),
    ] {
        let hit_all = group.iter().all(|s| stubs.iter().any(|x| x == s));
        let hit_any = group.iter().any(|s| stubs.iter().any(|x| x == s));
        if hit_all {
            let mut calls = BTreeMap::new();
            calls.insert(
                name.to_owned(),
                Call {
                    din: a.clone(),
                    dout: b.clone(),
                },
            );
            let replay = Replay { calls };
            match replay.apply_report(name, &env) {
                Ok(applied) => {
                    if !applied.notes.is_empty() {
                        notes.push(format!("{name}: {:?}", applied.notes));
                    }
                    blockers.retain(|x| !group.contains(&x.as_str()));
                    replayed.push(name);
                }
                Err(e) => notes.push(format!("replay {name}: {e}")),
            }
        } else if !hit_any {
            // ported: nothing to replay
        }
    }
    let mut mismatches = diff_with_unit_nondet(dout, &env, &after, &stem);
    // CCR-0030 row 7: `modList.notBuff` on a ModList object is not readable from the calc (the
    // frozen ModStore has no such field; the injected value lives in the probe registry), so
    // each such buff counts once too often in `BuffOnSelf` (CalcPerform.lua:3346-3348).
    let not_buff = count_not_buff(din);
    let mut known = Vec::new();
    mismatches.retain(|m| {
        let explained = not_buff > 0
            && m.path == "env.modDB.multipliers.BuffOnSelf"
            && m.kind == MismatchKind::Value
            && match (
                m.pob.as_ref().and_then(J::as_f64),
                m.ours.as_ref().and_then(J::as_f64),
            ) {
                (Some(p), Some(o)) => o - p == not_buff as f64,
                _ => false,
            };
        if explained {
            known.push(format!(
                "{}: pob={:?} ours={:?} ({not_buff} notBuff buff(s), CCR-0030)",
                m.path, m.pob, m.ours
            ));
        }
        !explained
    });
    // Replay limit (pob_oracle::replay module doc): a mod the callee changed in place (e.g.
    // `modLib.setSource` on `value.mod` of an `EnemyModifier` LIST mod inside doActorMisc) is
    // replayed as removed + appended, so its position in the store's list moves. A LUA-ORDER
    // mismatch on a list that a replayed callee changed in PoB is that artefact, not the
    // segment's order (the multiset matches, or the diff would report mod-missing/extra).
    let deltas: Vec<(J, J)> = replayed
        .iter()
        .map(|name| {
            if *name == "mergeFlasks" {
                (resolve_ids(&d3118.root), resolve_ids(&d3125.root))
            } else {
                (resolve_ids(&d3484.root), resolve_ids(&dout.root))
            }
        })
        .collect();
    mismatches.retain(|m| {
        let artefact = m.kind == MismatchKind::LuaOrder
            && deltas.iter().any(|(a, b)| {
                let (x, y) = (walk(a, &m.path), walk(b, &m.path));
                x.is_some() && y.is_some() && strip_at(x.unwrap()) != strip_at(y.unwrap())
            });
        if artefact {
            known.push(format!(
                "{} LUA-ORDER: list changed in place by a replayed callee (replay limit)",
                m.path
            ));
        }
        !artefact
    });
    let verdict = if mismatches.is_empty() {
        Verdict::Pass
    } else if blockers.is_empty() {
        Verdict::Fail
    } else {
        Verdict::Blocked
    };
    Outcome {
        verdict,
        mismatches,
        known,
        blockers,
        replayed,
        note: notes.join("; "),
    }
}

#[test]
fn pff_l4_perform_2997_3497() {
    if !testkit::init_default_data() {
        return skip("no data dump (data/dump)");
    }
    let Some(parity) = find_parity_dir() else {
        return skip("no .parity directory");
    };
    let builds = corpus(&parity);
    if builds.is_empty() {
        return skip("no builds");
    }
    let verbose = std::env::var_os("PFF_VERBOSE").is_some();
    let mut counts: BTreeMap<Verdict, usize> = BTreeMap::new();
    let mut blockers: BTreeMap<String, usize> = BTreeMap::new();
    let mut fails = Vec::new();
    let mut total_mm = 0usize;
    let mut order_only = 0usize;
    for xml in &builds {
        let o = run_build(xml);
        *counts.entry(o.verdict).or_default() += 1;
        total_mm += o.mismatches.len();
        let orders = o
            .mismatches
            .iter()
            .filter(|m| m.kind == MismatchKind::LuaOrder)
            .count();
        if orders > 0 && orders == o.mismatches.len() {
            order_only += 1;
        }
        for b in &o.blockers {
            *blockers.entry(b.clone()).or_default() += 1;
        }
        eprintln!(
            "{:<40} {:?} mismatches={} replayed={:?} blockers={:?} {}",
            stem(xml),
            o.verdict,
            o.mismatches.len(),
            o.replayed,
            o.blockers,
            o.note
        );
        let show = if verbose { usize::MAX } else if o.verdict == Verdict::Fail { 15 } else { 0 };
        for m in o.mismatches.iter().take(show) {
            eprintln!(
                "    {} {:?} pob={} ours={}",
                m.path,
                m.kind,
                clip(m.pob.as_ref()),
                clip(m.ours.as_ref())
            );
        }
        for k in &o.known {
            eprintln!("    known: {k}");
        }
        if o.verdict == Verdict::Fail {
            fails.push(stem(xml).to_owned());
        }
    }
    eprintln!(
        "PF-F L4 2997->3497: {} builds: pass={} blocked={} fail={} missing={} (mismatches {}, LUA-ORDER-only builds {})",
        builds.len(),
        counts.get(&Verdict::Pass).copied().unwrap_or(0),
        counts.get(&Verdict::Blocked).copied().unwrap_or(0),
        counts.get(&Verdict::Fail).copied().unwrap_or(0),
        counts.get(&Verdict::Missing).copied().unwrap_or(0),
        total_mm,
        order_only
    );
    eprintln!("blocking stubs (builds): {blockers:?}");
    let ran = builds.len() - counts.get(&Verdict::Missing).copied().unwrap_or(0);
    if ran == 0 {
        return skip("no perform:2997/3497 dumps (see the module doc for the oracle.sh command)");
    }
    assert!(fails.is_empty(), "PF-F L4 failures: {fails:?}");
    assert!(
        counts.get(&Verdict::Pass).copied().unwrap_or(0) > 0,
        "no build passed"
    );
}

/// `testkit::diff_against` with the unit's nondet record for the PF-F hits
/// (`.parity/unit/PF-F/nondet/<stem>.json`, written by `pob_oracle nondet --stages
/// corpus/PF-F/stages.toml --select perform:3497@main`), else the default record.
fn diff_with_unit_nondet(
    dump: &StateDump,
    env: &pob_calc::env::EnvRef,
    locals: &[(String, pob_calc::Val)],
    stem: &str,
) -> Vec<Mismatch> {
    let pob_locals: Vec<String> = dump
        .locals()
        .and_then(J::as_object)
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default();
    let kept: Vec<(String, pob_calc::Val)> = locals
        .iter()
        .filter(|(k, _)| pob_locals.contains(k))
        .cloned()
        .collect();
    let ours = testkit::export_after(Some(env), &kept);
    let mut pob = dump.root.clone();
    if let Some(J::Object(l)) = pob.get_mut("locals") {
        let names: Vec<&String> = kept.iter().map(|(k, _)| k).collect();
        l.retain(|k, v| {
            names.contains(&k) || !v.as_object().is_some_and(|m| m.contains_key("$fn"))
        });
    }
    let parity = find_parity_dir();
    let rec = parity.as_ref().and_then(|p| {
        [
            p.join("unit/PF-F/nondet").join(format!("{stem}.json")),
            p.join("nondet").join(format!("{stem}.json")),
        ]
        .into_iter()
        .filter(|f| f.is_file())
        .find_map(|f| {
            NondetRecord::load(&f).ok().filter(|r| {
                r.states.contains_key(&dump.hit) || r.state_orders.contains_key(&dump.hit)
            })
        })
    });
    let opts = DiffOptions {
        nondet_values: rec.as_ref().and_then(|n| n.states.get(&dump.hit)),
        nondet_orders: rec.as_ref().and_then(|n| n.state_orders.get(&dump.hit)),
        ..DiffOptions::strict()
    };
    diff_trees(&resolve_ids(&pob), &resolve_ids(&ours), &opts)
}

/// Buffs of the `:in` state whose ModList carries `notBuff = true`
/// (CalcPerform.lua 2331, 2810, 2834).
fn count_not_buff(din: &StateDump) -> usize {
    let root = resolve_ids(&din.root);
    root.get("env")
        .and_then(|e| e.get("buffs"))
        .and_then(J::as_object)
        .map_or(0, |m| {
            m.values()
                .filter(|v| v.get("notBuff").and_then(J::as_bool) == Some(true))
                .count()
        })
}

/// The value at a diff path (`env.modDB.mods.Damage`, `env.buffs["Molten Shell"]`, `a[2]`)
/// in a resolved root; a store's `mods.<name>` is its list of mods of that name.
fn walk<'a>(root: &'a J, path: &str) -> Option<&'a J> {
    let mut v = root;
    let mut rest = path;
    while !rest.is_empty() {
        let key: String;
        if let Some(r) = rest.strip_prefix("[\"") {
            let end = r.find("\"]")?;
            key = r[..end].to_owned();
            rest = &r[end + 2..];
        } else if let Some(r) = rest.strip_prefix('[') {
            let end = r.find(']')?;
            let i: usize = r[..end].parse().ok()?;
            rest = &r[end + 1..];
            v = match v {
                J::Array(a) => a.get(i.checked_sub(1)?)?,
                J::Object(m) => m.get("$seq")?.as_array()?.get(i.checked_sub(1)?)?,
                _ => return None,
            };
            continue;
        } else {
            let r = rest.strip_prefix('.').unwrap_or(rest);
            let end = r.find(['.', '[']).unwrap_or(r.len());
            key = r[..end].to_owned();
            rest = &r[end..];
        }
        v = v.as_object()?.get(&key)?;
    }
    Some(v)
}

/// A JSON value without mod provenance (`$at`).
fn strip_at(v: &J) -> J {
    match v {
        J::Object(m) => J::Object(
            m.iter()
                .filter(|(k, _)| k.as_str() != "$at")
                .map(|(k, x)| (k.clone(), strip_at(x)))
                .collect(),
        ),
        J::Array(a) => J::Array(a.iter().map(strip_at).collect()),
        other => other.clone(),
    }
}

fn clip(v: Option<&serde_json::Value>) -> String {
    let s = v.map_or_else(|| "-".to_owned(), ToString::to_string);
    if s.len() > 160 {
        format!("{}…", &s[..s.char_indices().nth(160).map_or(s.len(), |(i, _)| i)])
    } else {
        s
    }
}
