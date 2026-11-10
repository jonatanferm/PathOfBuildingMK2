//! U15 acceptance (ui doc §12 U15): the calc facade.
//!
//! * `bridge_build_output_matches_pob`: for the 6 fixture builds, after `build_output` through
//!   the `LuaCalcBridge`, `build.calcsTab.mainOutput` / `calcsOutput` and the breakdowns (player
//!   and minion) equal PoB's. The reference is an independent LuaJIT PoB run on the fixture's
//!   own XML (the bridge loads the Rust model's `SaveDB` XML), dumped by the same encoder, and
//!   compared with `pob_oracle::diff::diff_trees` (numbers to 1e-9 relative) against the Rust
//!   export (`pob_calc::probe::export_root`) of what the bridge adopted.
//! * `bridge_misc_calc_matches_pob`: `MiscCalc::call` with addNodes, removeNodes, repSlotName +
//!   repItem (a live item and a fresh one) and no override equals Lua `calcFunc` with the same
//!   override written by hand in Lua.
//! * `rust_calc_runs_on_stubs`: `RustCalc` against the calc stubs (calc doc D9).
//!
//! One LuaJIT PoB at a time (the bridge holds the oracle lock per operation); run with
//! `--test-threads=1` to keep one build's state in memory at a time.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::task::Poll;

use pob_app::calc::{CalcFacade, RustCalc};
use pob_calc::env::{EnvRef, Override};
use pob_calc::lua_table::{Output, Val};
use pob_calc::model::build_xml::loadBuildModelFromXML;
use pob_calc::model::BuildRef;
use pob_lua::{LjIntMap, Shared};
use pob_oracle::diff::{
    diff_trees, tree_eq, tree_eq_unordered, DiffOptions, Mismatch, MismatchKind, REL_TOL,
};
use pob_oracle::schema::{get_path, resolve_ids};
use pob_ui_oracle::lua_calc::{LuaCalcBridge, LuaSession};
use serde_json::Value as J;

fn fixtures() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/user1/Path of Building/Builds");
    let mut v: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "xml"))
        .collect();
    v.sort();
    assert_eq!(v.len(), 6, "the 6 fixture builds");
    v
}

/// PoB's src and the data dump, or None (skip policy of pob-testpaths).
fn inputs() -> Option<PathBuf> {
    let src = pob_testpaths::require(
        pob_testpaths::pob_src(),
        "PoB checkout (.PathOfBuilding/src)",
    )?;
    let dump = pob_testpaths::require(pob_testpaths::dump_dir(), "data/dump")?;
    assert!(pob_oracle::testkit::init_data(&dump), "data dump");
    // Timeless jewel LUTs load from the PoB checkout (as tests/BM-A.rs sets it up).
    let src = std::fs::canonicalize(&src).unwrap_or(src);
    pob_gamedata::timeless_lut::set_jewel_fs(std::sync::Arc::new(
        pob_gamedata::timeless_lut::StdJewelFs {
            script_path: src.to_string_lossy().into_owned(),
            writable: false,
        },
    ));
    Some(src)
}

fn load(xml: &str) -> BuildRef {
    loadBuildModelFromXML(xml, pob_gamedata::data()).expect("BM-A loads the fixture")
}

/// `{"$ref": path}` → the referenced subtree.
fn deref<'a>(root: &'a J, v: &'a J) -> &'a J {
    match v.get("$ref").and_then(J::as_str) {
        Some(p) => get_path(root, p).unwrap_or(v),
        None => v,
    }
}

fn sub<'a>(root: &'a J, path: &str) -> &'a J {
    static NULL: J = J::Null;
    get_path(root, path).map_or(&NULL, |v| deref(root, v))
}

/// The mismatches under `path`, split into real ones and two kinds PoB itself produces between
/// two processes (§9.5): lists whose elements agree as a multiset but not in order (`pairs`
/// order; `.parity/nondet` records such lists for every corpus build), and identity references
/// whose canonical path differs while the referenced values agree. A third category is the
/// export's: a typed Rust field (`bool`, `cfg.flags`) exports `false`/`0` where PoB has nil
/// unless the probe registry marked it absent, and the registry is reset by every injection
/// (CCR-0030, CCR-0088 §2), so a live model object adopted from an earlier document exports its
/// default.
struct Gate {
    hard: Vec<Mismatch>,
    order_only: usize,
    ref_only: usize,
    nil_default: usize,
}

fn diff_at(pob: &J, ours: &J, path: &str) -> Gate {
    let (sp, so) = (sub(pob, path), sub(ours, path));
    let mut g = Gate {
        hard: Vec::new(),
        order_only: 0,
        ref_only: 0,
        nil_default: 0,
    };
    for m in diff_trees(sp, so, &DiffOptions::strict()) {
        let typed_default = match &m.ours {
            Some(J::Bool(false)) => true,
            Some(J::Number(n)) => n.as_f64() == Some(0.0),
            _ => false,
        };
        if m.kind == MismatchKind::Extra && m.pob.is_none() && typed_default {
            g.nil_default += 1;
            continue;
        }
        let is_ref = |v: &Option<J>| v.as_ref().is_some_and(|v| v.get("$ref").is_some());
        if is_ref(&m.pob) && is_ref(&m.ours) {
            let (a, b) = (m.pob.as_ref().unwrap(), m.ours.as_ref().unwrap());
            if tree_eq(deref(pob, a), deref(ours, b), REL_TOL) {
                g.ref_only += 1;
                continue;
            }
        }
        // the enclosing lists, innermost first
        let mut order = false;
        let mut end = m.path.len();
        while let Some(i) = m.path[..end].rfind('[') {
            let list = &m.path[..i];
            if m.path[i + 1..].starts_with(|c: char| c.is_ascii_digit()) {
                if let (Some(a), Some(b)) = (get_path(sp, list), get_path(so, list)) {
                    if tree_eq_unordered(a, b, REL_TOL) {
                        order = true;
                        break;
                    }
                }
            }
            end = i;
        }
        if order {
            g.order_only += 1;
        } else {
            g.hard.push(m);
        }
    }
    g
}

fn show(ms: &[Mismatch]) -> String {
    ms.iter()
        .take(12)
        .map(|m| {
            format!(
                "  {} {:?}: pob={:?} ours={:?}",
                m.path, m.kind, m.pob, m.ours
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The pob-state root of `env` as the Rust graph exports it, ids resolved.
fn ours_env(env: &EnvRef) -> J {
    resolve_ids(&pob_calc::probe::export_root(Some(env), &[]))
}

#[test]
fn bridge_build_output_matches_pob() {
    let Some(src) = inputs() else { return };
    let mut gate_fail = Vec::new();
    let mut totals = (0usize, 0usize, 0usize, 0usize, 0usize);
    for path in fixtures() {
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        let xml = std::fs::read_to_string(&path).unwrap();
        let t0 = std::time::Instant::now();
        let build = load(&xml);
        let mut bridge = LuaCalcBridge::new(Some(src.clone()));
        assert!(matches!(bridge.build_output(&build), Poll::Ready(())));
        let t_bridge = t0.elapsed();
        let ct = &build.calcsTab;
        let main_env = ct.mainEnv.get().expect("mainEnv");
        let calcs_env = ct.calcsEnv.get().expect("calcsEnv");
        assert!(ct
            .mainOutput
            .get()
            .expect("mainOutput")
            .ptr_eq(&main_env.player.output.get()));
        assert!(ct
            .calcsOutput
            .get()
            .expect("calcsOutput")
            .ptr_eq(&calcs_env.player.output.get()));
        assert!(ct.powerBuildFlag.get());
        // the live socket groups are the ones the injected skills point at
        let groups = build.skillsTab.socketGroupList.get().snapshot();
        for s in main_env.player.activeSkillList.get().snapshot() {
            if let Some(g) = s.socketGroup.get() {
                assert!(
                    groups.iter().any(|l| Rc::ptr_eq(l, &g)),
                    "{stem}: an active skill's socketGroup is not a live group"
                );
            }
            if let Some(gem) = s.activeEffect.get().srcInstance.get() {
                assert!(
                    groups.iter().any(|l| l
                        .gemList
                        .get()
                        .snapshot()
                        .iter()
                        .any(|x| Rc::ptr_eq(x, &gem))),
                    "{stem}: an active skill's srcInstance is not a live gem"
                );
            }
        }
        let rep = bridge.last_report.clone().unwrap();

        // reference: PoB on the fixture's own XML
        let t1 = std::time::Instant::now();
        let docs = {
            let mut s = LuaSession::load(&src, &xml, "Unnamed build").expect("reference load");
            s.dump_build_output().expect("reference dump")
        };
        let t_ref = t1.elapsed();
        let mut line = format!(
            "{stem}: bridge {:.1}s ref {:.1}s; groups kept/new {}/{}, gems {}/{}, rebound {}, finalModList {}, fallbacks {}",
            t_bridge.as_secs_f64(),
            t_ref.as_secs_f64(),
            rep.groups_kept,
            rep.groups_new,
            rep.gems_kept,
            rep.gems_new,
            rep.rebound,
            rep.nodes_final,
            rep.fallbacks.values().sum::<u64>()
        );
        for (which, doc, env) in [
            ("main", &docs.main, &main_env),
            ("calcs", &docs.calcs, &calcs_env),
        ] {
            let pob = resolve_ids(&doc.root);
            let ours = ours_env(env);
            for p in [
                "env.player.output",
                "env.player.breakdown",
                "env.minion.output",
                "env.minion.breakdown",
            ] {
                let g = diff_at(&pob, &ours, p);
                let n = flat_len(sub(&pob, p));
                let _ = write!(
                    line,
                    "\n    {which} {p}: {n} leaves, {} mismatches (+{} order-only, {} ref-path-only, {} nil-default)",
                    g.hard.len(),
                    g.order_only,
                    g.ref_only,
                    g.nil_default
                );
                totals.0 += n;
                totals.1 += g.hard.len();
                totals.2 += g.order_only;
                totals.3 += g.ref_only;
                totals.4 += g.nil_default;
                if !g.hard.is_empty() {
                    gate_fail.push(format!("{stem} {which} {p}:\n{}", show(&g.hard)));
                }
            }
            let all = diff_at(&pob, &ours, "env");
            let _ = write!(
                    line,
                "\n    {which} env (informational): {} leaves, {} mismatches (+{} order-only, {} ref-path-only, {} nil-default)",
                flat_len(sub(&pob, "env")),
                all.hard.len(),
                all.order_only,
                all.ref_only,
                all.nil_default
            );
            if !all.hard.is_empty() {
                let _ = write!(line, "\n{}", show(&all.hard));
            }
        }
        // the misc calculator's base output (CalcsTab.lua:470-471)
        let (_, base) = match bridge.misc_calculator(&build) {
            Poll::Ready(x) => x,
            Poll::Pending => unreachable!(),
        };
        // its model objects are the live ones (`ReqStrItem.sourceGem`)
        let mut live_gems = 0;
        for (_, v) in base.pairs() {
            if let Some(gem) = v
                .table()
                .and_then(|t| t.obj::<pob_calc::model::GemInstance>("sourceGem"))
            {
                assert!(
                    groups.iter().any(|l| l
                        .gemList
                        .get()
                        .snapshot()
                        .iter()
                        .any(|x| Rc::ptr_eq(x, &gem))),
                    "{stem}: miscCalcBase sourceGem is not a live gem"
                );
                live_gems += 1;
            }
        }
        let pob = resolve_ids(&docs.main.root);
        let ours = resolve_ids(&pob_calc::probe::export_root(
            Some(&main_env),
            &[
                ("miscCalcBase".into(), Val::Table(base)),
                ("socketGroupList".into(), groups_val(&build)),
            ],
        ));
        let g = diff_at(&pob, &ours, "locals.miscCalcBase");
        let n = flat_len(sub(&pob, "locals.miscCalcBase"));
        let _ = write!(
                    line,
            "\n    miscCalcBase: {n} leaves, {} mismatches (+{} order-only, {} ref-path-only, {} nil-default); {live_gems} live sourceGems",
            g.hard.len(),
            g.order_only,
            g.ref_only,
            g.nil_default
        );
        totals.0 += n;
        totals.1 += g.hard.len();
        totals.2 += g.order_only;
        totals.3 += g.ref_only;
        totals.4 += g.nil_default;
        if !g.hard.is_empty() {
            gate_fail.push(format!("{stem} miscCalcBase:\n{}", show(&g.hard)));
        }
        println!("{line}");
    }
    println!(
        "TOTAL gated leaves {}: {} mismatches, {} order-only, {} ref-path-only, {} nil-default",
        totals.0, totals.1, totals.2, totals.3, totals.4
    );
    assert!(
        gate_fail.is_empty(),
        "mismatches:\n{}",
        gate_fail.join("\n")
    );
}

/// `build.skillsTab.socketGroupList` as the dynamic-table value a document local holds.
fn groups_val(build: &BuildRef) -> Val {
    let t = pob_calc::lua_table::Table::new();
    for g in build.skillsTab.socketGroupList.get().snapshot() {
        t.push(Val::Obj(g));
    }
    Val::Table(t)
}

fn flat_len(v: &J) -> usize {
    pob_oracle::schema::flatten_state(v).len()
}

/// An unallocated notable next to the allocated tree and an allocated notable, both with mods
/// (the first by id).
fn pick_nodes(build: &BuildRef) -> (f64, f64) {
    let spec = build.spec.get();
    let mut nodes: Vec<_> = spec.nodes.with(LjIntMap::pairs);
    nodes.sort_by_key(|(id, _)| *id);
    let plain = |n: &pob_calc::classes::Node| {
        n.ascendancyName.get().is_none()
            && !n.isMastery.get()
            && !n.isJewelSocket.get()
            && !n.isKeystone.get()
            && n.classStartIndex.get().is_none()
            && !n.modList.get().is_empty()
    };
    let add = nodes
        .iter()
        .find(|(_, n)| {
            !n.alloc.get()
                && plain(n)
                && n.isNotable.get()
                && n.linked.get().snapshot().iter().any(|l| l.alloc.get())
        })
        .map(|(_, n)| n.id)
        .expect("an unallocated notable");
    let rem = nodes
        .iter()
        .find(|(_, n)| n.alloc.get() && plain(n) && n.isNotable.get())
        .map(|(_, n)| n.id)
        .expect("an allocated notable");
    (add, rem)
}

fn node_map(build: &BuildRef, id: f64) -> Shared<LjIntMap<pob_calc::classes::NodeRef>> {
    let node = build
        .spec
        .get()
        .nodes
        .with(|m| m.get(id as i64))
        .expect("node");
    let mut m = LjIntMap::new();
    m.set(id as i64, Some(node));
    Shared::new(m)
}

/// The item equipped in `slot`.
fn slot_item(build: &BuildRef, slot: &str) -> Option<pob_calc::classes::ItemRef> {
    let id = build.itemsTab.slots.with(|m| {
        m.get(&pob_lua::Name::from(slot))
            .map(|t| t.num_or("selItemId", 0.0))
    })?;
    if id == 0.0 {
        return None;
    }
    build.itemsTab.items.with(|m| m.get(&(id as u32)).cloned())
}

fn call(
    calc: &Rc<dyn pob_app::calc::MiscCalc>,
    ov: Option<Rc<Override>>,
    full: Option<bool>,
) -> Output {
    match pob_lua::error::catch(|| calc.call(ov, full)) {
        Ok(Poll::Ready(o)) => o,
        Ok(Poll::Pending) => unreachable!(),
        Err(e) => panic!("MiscCalc::call raised a Lua error: {}", e.0),
    }
}

/// (name, override, useFullDPS, the same override as a Lua function body).
type Case = (String, Option<Rc<Override>>, Option<bool>, String);

#[test]
fn bridge_misc_calc_matches_pob() {
    let Some(src) = inputs() else { return };
    let mut fails = Vec::new();
    let mut cases_run = 0;
    let mut totals = (0usize, 0usize, 0usize, 0usize, 0usize);
    // three fixtures (each case = two PoB boots)
    for path in fixtures().into_iter().step_by(2) {
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        let xml = std::fs::read_to_string(&path).unwrap();
        let build = load(&xml);
        let mut bridge = LuaCalcBridge::new(Some(src.clone()));
        assert!(matches!(bridge.build_output(&build), Poll::Ready(())));
        let (calc, _) = match bridge.misc_calculator(&build) {
            Poll::Ready(x) => x,
            Poll::Pending => unreachable!(),
        };
        let (add, rem) = pick_nodes(&build);
        let mut cases: Vec<Case> = vec![
            ("none".into(), None, None, "return nil".into()),
            (
                format!("addNodes {{{add}}}"),
                Some(Rc::new({
                    let o = Override::default();
                    o.addNodes.set(Some(node_map(&build, add)));
                    o
                })),
                Some(true),
                format!("return {{ addNodes = {{ [b.spec.nodes[{add}]] = true }} }}"),
            ),
            (
                format!("removeNodes {{{rem}}}"),
                Some(Rc::new({
                    let o = Override::default();
                    o.removeNodes.set(Some(node_map(&build, rem)));
                    o
                })),
                Some(false),
                format!("return {{ removeNodes = {{ [b.spec.nodes[{rem}]] = true }} }}"),
            ),
        ];
        // repSlotName + repItem: the Ring 2 item in Ring 1 (a live item, by identity), and a
        // fresh copy of the body armour (crosses as raw text)
        if let Some(ring) = slot_item(&build, "Ring 2") {
            let id = ring.id.get().unwrap();
            cases.push((
                "repSlotName Ring 1 / live Ring 2 item".into(),
                Some(Rc::new({
                    let o = Override::default();
                    o.repSlotName.set(Some(pob_lua::Name::from("Ring 1")));
                    o.repItem.set(Some(ring.clone()));
                    o
                })),
                None,
                format!("return {{ repSlotName = \"Ring 1\", repItem = b.itemsTab.items[{id}] }}"),
            ));
        }
        if let Some(body) = slot_item(&build, "Body Armour") {
            let id = body.id.get().unwrap();
            let fresh = pob_calc::classes::item::new_Item(Some(&body.BuildRaw()), None, false);
            cases.push((
                "repSlotName Body Armour / fresh item".into(),
                Some(Rc::new({
                    let o = Override::default();
                    o.repSlotName.set(Some(pob_lua::Name::from("Body Armour")));
                    o.repItem.set(Some(fresh));
                    o
                })),
                None,
                format!(
                    "return {{ repSlotName = \"Body Armour\", repItem = new(\"Item\"):Item(b.itemsTab.items[{id}]:BuildRaw()) }}"
                ),
            ));
        }
        let mut none_out: Option<J> = None;
        for (name, ov, full, lua_body) in cases {
            let ours = call(&calc, ov, full);
            let pob = {
                let mut s = LuaSession::load(&src, &xml, "Unnamed build").expect("reference load");
                s.call_misc(&lua_body, full).expect("reference calcFunc")
            };
            let pob_root = resolve_ids(&pob.root);
            let ours_root = resolve_ids(&pob_calc::probe::export_root(
                None,
                &[
                    ("output".into(), Val::Table(ours)),
                    ("socketGroupList".into(), groups_val(&build)),
                ],
            ));
            let g = diff_at(&pob_root, &ours_root, "locals.output");
            let leaves = flat_len(sub(&pob_root, "locals.output"));
            // the override must matter in PoB (else the case proves nothing)
            let pob_out = sub(&pob_root, "locals.output").clone();
            let changed = match &none_out {
                None => {
                    none_out = Some(pob_out);
                    0
                }
                Some(base) => diff_trees(base, &pob_out, &DiffOptions::strict()).len(),
            };
            if name.starts_with("addNodes")
                || name.starts_with("removeNodes")
                || name.contains("Ring")
            {
                assert!(
                    changed > 0,
                    "{stem} {name}: PoB's output equals the no-override output"
                );
            }
            println!(
                "{stem} {name}: {leaves} leaves ({changed} differ from no override), {} mismatches (+{} order-only, {} ref-path-only, {} nil-default)",
                g.hard.len(),
                g.order_only,
                g.ref_only,
                g.nil_default
            );
            assert!(
                leaves > 100,
                "{stem} {name}: the reference output is too small"
            );
            cases_run += 1;
            totals.0 += leaves;
            totals.1 += g.hard.len();
            totals.2 += g.order_only;
            totals.3 += g.ref_only;
            totals.4 += g.nil_default;
            if !g.hard.is_empty() {
                fails.push(format!("{stem} {name}:\n{}", show(&g.hard)));
            }
        }
    }
    println!(
        "TOTAL misc cases {cases_run}, leaves {}: {} mismatches, {} order-only, {} ref-path-only, {} nil-default",
        totals.0, totals.1, totals.2, totals.3, totals.4
    );
    assert!(fails.is_empty(), "mismatches:\n{}", fails.join("\n"));
}

#[test]
fn rust_calc_runs_on_stubs() {
    let Some(_) = inputs() else { return };
    let path = &fixtures()[0];
    let build = load(&std::fs::read_to_string(path).unwrap());
    let mut calc = RustCalc;
    assert!(matches!(calc.build_output(&build), Poll::Ready(())));
    let (f, _base) = match calc.misc_calculator(&build) {
        Poll::Ready(x) => x,
        Poll::Pending => unreachable!(),
    };
    let (add, rem) = pick_nodes(&build);
    let ov = Override::default();
    ov.addNodes.set(Some(node_map(&build, add)));
    ov.removeNodes.set(Some(node_map(&build, rem)));
    assert!(matches!(
        f.call(Some(Rc::new(ov)), Some(true)),
        Poll::Ready(_)
    ));
    assert!(matches!(f.call(None, None), Poll::Ready(_)));
    let report = pob_calc::stub::stub_report();
    println!("stub sites hit: {report:?}");
}

/// Diagnostics (`--ignored`): PoB against itself, two VMs on the same fixture XML, with the
/// gate's categories, to show which order-only differences are PoB's own (§9.5).
#[test]
#[ignore = "diagnostics"]
fn pob_against_itself() {
    let Some(src) = inputs() else { return };
    for path in fixtures() {
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        let xml = std::fs::read_to_string(&path).unwrap();
        let run = || {
            let mut s = LuaSession::load(&src, &xml, "Unnamed build").expect("load");
            s.dump_build_output().expect("dump")
        };
        let (first, second) = (run(), run());
        for (which, d1, d2) in [
            ("main", &first.main, &second.main),
            ("calcs", &first.calcs, &second.calcs),
        ] {
            let (x, y) = (resolve_ids(&d1.root), resolve_ids(&d2.root));
            for p in ["env.player.output", "env.player.breakdown"] {
                let g = diff_at(&x, &y, p);
                println!(
                    "{stem} {which} {p}: {} mismatches, {} order-only, {} ref-path-only, {} nil-default",
                    g.hard.len(),
                    g.order_only,
                    g.ref_only,
                    g.nil_default
                );
            }
        }
    }
}
