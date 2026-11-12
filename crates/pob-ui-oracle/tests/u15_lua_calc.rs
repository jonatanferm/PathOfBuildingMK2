//! U15 acceptance (ui doc §12 U15): the calc facade.
//!
//! * `bridge_build_output_matches_pob`: for the 6 fixture builds, after `build_output` through
//!   the `LuaCalcBridge`, `build.calcsTab.mainOutput` / `calcsOutput` and the breakdowns (player
//!   and minion) equal PoB's. The reference is an independent LuaJIT PoB run on the fixture's
//!   own XML (the bridge loads the Rust model's `SaveDB` XML), dumped by the same encoder, and
//!   compared with `pob_oracle::diff::diff_trees` (numbers to 1e-9 relative) against the Rust
//!   export (`pob_calc::probe::export_root`) of what the bridge adopted.
//!   It also checks the calc's other model writes as PoB left them (`mainSocketGroup`,
//!   `skill_number`, the items' `jewelRadiusData` / `jewelData.limitDisabled`) and that the
//!   envs' items (`flasks` / `tinctures` keys, `player.itemList`) are the live ones.
//! * `bridge_misc_calc_matches_pob`: `MiscCalc::call` with addNodes, removeNodes, repSlotName +
//!   repItem (a live item and a fresh one), spec (a second tree spec), toggleFlask /
//!   toggleTincture (live items), conditions (copied back) and no override equals Lua
//!   `calcFunc` with the same override written by hand in Lua.
//! * `bridge_model_writes`: a removed group stops being `displayGroup`; out-of-range
//!   `mainSocketGroup` / `skill_number` are clamped.
//!
//! The reference runs load [`reference_xml`] (old-format Vaal gems pinned to one variant).
//! The order-only exemption is not limited to `.parity/nondet`'s paths: those records cover
//! the corpus builds' probe states (their `stateOrders` hold almost no `breakdown` and no
//! `output` paths, and none of the `upstream_*` fixtures), while the order-only differences
//! here are breakdown rows.
//! * `rust_calc_runs_on_stubs`: `RustCalc` against the calc stubs (calc doc D9).
//!
//! One LuaJIT PoB at a time (the bridge holds the oracle lock per operation); run with
//! `--test-threads=1` to keep one build's state in memory at a time.
//!
//! TP2: the bridge's and the reference's Lua sessions go through the Lua-side cache
//! (`LuaSession::cached_*`, `pob_ui_oracle::oracle_cache`), and the fixtures of the two gate
//! tests run as independent cases on `oracle_cache::par_map` (at most `POB_TEST_PAR` at once in
//! the process, a fresh thread per fixture); lines and failures are collected in fixture order.

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

/// The reference XML: `xml` with `variantId` pinned on old-format gems whose variant PoB picks
/// by `pairs` order. SkillsTab.lua:472-477 resolves a `<Gem gemId skillId>` without `variantId`
/// by the first variant whose `grantedEffectId` is `skillId`; a Vaal gem and its AltX/AltY
/// variants share both (Data.lua:1126-1146), and LuaJIT's randomised string hashing makes the
/// pick differ between processes (e.g. `upstream_Dual_Wield_Cospris_CoC`'s "Vaal Righteous
/// Fire" loads as "... of Arcane Devotion" in some runs). The bridge's VM loads the model's
/// SaveDB XML, which carries the `variantId` BM-A resolved (the first in data order, the Rust
/// port's LUA-ORDER choice), so the reference is pinned to the same variant.
fn reference_xml(xml: &str) -> String {
    let data = pob_gamedata::data();
    let attr = |tag: &str, name: &str| -> Option<String> {
        let key = format!(" {name}=\"");
        let a = tag.find(&key)? + key.len();
        let b = tag[a..].find('"')? + a;
        Some(tag[a..b].to_owned())
    };
    let mut out = String::with_capacity(xml.len());
    let mut rest = xml;
    while let Some(i) = rest.find("<Gem ") {
        out.push_str(&rest[..i]);
        let end = rest[i..].find('>').map_or(rest.len(), |e| i + e);
        let tag = &rest[i..end];
        let pinned = (|| {
            if attr(tag, "variantId").is_some() {
                return None;
            }
            let (gem_id, skill_id) = (attr(tag, "gemId")?, attr(tag, "skillId")?);
            let variants = data
                .gemsByGameId
                .0
                .get(&pob_lua::Name::from(gem_id.as_str()))?;
            let matches: Vec<_> = variants
                .values()
                .filter(|v| v.grantedEffectId.as_str() == skill_id)
                .collect();
            (matches.len() > 1).then(|| matches[0].variantId.to_string())
        })();
        out.push_str("<Gem ");
        if let Some(v) = pinned {
            let _ = write!(out, "variantId=\"{v}\" ");
        }
        out.push_str(&tag["<Gem ".len()..]);
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
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
/// default. That category is only excused for a key that is a typed field of a reflected
/// object ([`typed_keys`]): a dynamic key holding 0/false where PoB has nil is a real
/// difference (0 is truthy in Lua).
struct Gate {
    hard: Vec<Mismatch>,
    order_only: usize,
    ref_only: usize,
    nil_default: usize,
}

/// The typed field names of the reflected Rust objects (`pob_calc::probe::Reflect::KEYS`, and
/// the item fields the encoder writes).
fn typed_keys() -> &'static std::collections::HashSet<&'static str> {
    use pob_calc::probe::Reflect;
    static KEYS: std::sync::OnceLock<std::collections::HashSet<&'static str>> =
        std::sync::OnceLock::new();
    KEYS.get_or_init(|| {
        [
            pob_calc::env::Env::KEYS,
            pob_calc::actor::Actor::KEYS,
            pob_calc::active_skill::ActiveSkill::KEYS,
            pob_calc::active_skill::SkillEffect::KEYS,
            pob_calc::cfg::ModCfg::KEYS,
            pob_calc::model::GemInstance::KEYS,
            pob_calc::model::SocketGroup::KEYS,
            pob_calc::env::Override::KEYS,
            pob_calc::probe::ITEM_FIELDS,
        ]
        .concat()
        .into_iter()
        .collect()
    })
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
        let leaf = m.path.rsplit('.').next().unwrap_or("");
        let typed_field = !leaf.contains('[') && typed_keys().contains(leaf);
        if typed_field && m.kind == MismatchKind::Extra && m.pob.is_none() && typed_default {
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

/// The pob-state root of `env` as the Rust graph exports it, with the live
/// `socketGroupList` as the documents' local (so group references resolve to the same paths
/// as in PoB's document), ids resolved.
fn ours_env(env: &EnvRef, build: &BuildRef) -> J {
    resolve_ids(&pob_calc::probe::export_root(
        Some(env),
        &[("socketGroupList".into(), groups_val(build))],
    ))
}

#[test]
fn bridge_build_output_matches_pob() {
    let Some(src) = inputs() else { return };
    let results = pob_ui_oracle::oracle_cache::par_map(fixtures(), |path| {
        let mut gate_fail = Vec::new();
        let mut totals = (0usize, 0usize, 0usize, 0usize, 0usize);
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
        // the envs' items are the live ones (ItemsTab.lua:4868 `mainEnv.flasks[item]`, :4904)
        let (live_flags, live_slots) = assert_live_items(&build, &stem, &[&main_env, &calcs_env]);

        // reference: PoB on the fixture's own XML
        let t1 = std::time::Instant::now();
        // through the Lua-side cache (TP2): the same boot, load and dump as a fresh session
        let docs = LuaSession::cached_build_output(&src, &reference_xml(&xml), "Unnamed build")
            .expect("reference load and dump");
        let t_ref = t1.elapsed();
        let mut line = format!(
            "{stem}: bridge {:.1}s ref {:.1}s; groups kept/new {}/{}, gems {}/{}, rebound {}, items rebound {} ({live_flags} flask/tincture keys, {live_slots} itemList), finalModList {}, fallbacks {}",
            t_bridge.as_secs_f64(),
            t_ref.as_secs_f64(),
            rep.groups_kept,
            rep.groups_new,
            rep.gems_kept,
            rep.gems_new,
            rep.rebound,
            rep.items_rebound,
            rep.nodes_final,
            rep.fallbacks.values().sum::<u64>()
        );
        // the calc's writes into the model (CalcSetup.lua:1586, :1589, :293-298, :852/:866)
        let pob_main = resolve_ids(&docs.main.root);
        let num = |p: &str| sub(&pob_main, p).as_f64();
        assert_eq!(
            Some(build.mainSocketGroup.get()),
            num("locals.mainSocketGroup"),
            "{stem}: build.mainSocketGroup"
        );
        assert_eq!(
            build.calcsTab.input.opt_num("skill_number"),
            num("locals.skillNumber"),
            "{stem}: calcsTab.input.skill_number"
        );
        let (radius, limit) = live_item_writes(&build);
        let ours_items = resolve_ids(&pob_calc::probe::export_root(
            None,
            &[
                ("itemRadius".into(), Val::Table(radius.clone())),
                ("itemLimit".into(), Val::Table(limit.clone())),
            ],
        ));
        for p in ["locals.itemRadius", "locals.itemLimit"] {
            let g = diff_at(&pob_main, &ours_items, p);
            let _ = write!(
                line,
                "\n    {p}: {} leaves, {} mismatches (+{} order-only, {} ref-path-only, {} nil-default)",
                flat_len(sub(&pob_main, p)),
                g.hard.len(),
                g.order_only,
                g.ref_only,
                g.nil_default
            );
            if !g.hard.is_empty() {
                gate_fail.push(format!("{stem} {p}:\n{}", show(&g.hard)));
            }
        }
        for (which, doc, env) in [
            ("main", &docs.main, &main_env),
            ("calcs", &docs.calcs, &calcs_env),
        ] {
            let pob = resolve_ids(&doc.root);
            let ours = ours_env(env, &build);
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
        (line, gate_fail, totals)
    });
    let mut gate_fail = Vec::new();
    let mut totals = (0usize, 0usize, 0usize, 0usize, 0usize);
    for (line, g, t) in results {
        println!("{line}");
        gate_fail.extend(g);
        totals.0 += t.0;
        totals.1 += t.1;
        totals.2 += t.2;
        totals.3 += t.3;
        totals.4 += t.4;
    }
    println!("{}", pob_ui_oracle::oracle_cache::summary());
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

/// Every item key of `env.flasks` / `env.tinctures` and every `actor.itemList` entry with an id
/// is `build.itemsTab.items[id]` (returns the counts checked).
fn assert_live_items(build: &BuildRef, stem: &str, envs: &[&EnvRef]) -> (usize, usize) {
    let live = |it: &pob_calc::classes::ItemRef| {
        it.id.get().is_some_and(|id| {
            build
                .itemsTab
                .items
                .with(|m| m.get(&(id as u32)).is_some_and(|l| Rc::ptr_eq(l, it)))
        })
    };
    let (mut flags, mut slots) = (0, 0);
    for env in envs {
        for m in [env.flasks.get(), env.tinctures.get()] {
            for (it, _) in m.with(pob_lua::PtrMap::pairs) {
                assert!(
                    live(&it),
                    "{stem}: an env.flasks/tinctures key is not a live item"
                );
                flags += 1;
            }
        }
        let list: Vec<_> = env
            .player
            .itemList()
            .with(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect());
        for (slot, it) in list {
            if it.id.get().is_some() {
                assert!(
                    live(&it),
                    "{stem}: env.player.itemList[{slot}] is not a live item"
                );
                slots += 1;
            }
        }
    }
    (flags, slots)
}

/// `{ [id] = item.jewelRadiusData }` and `{ [id] = limitDisabled }` (jewels of allocated
/// sockets) of the live items, shaped like the bridge's `itemRadius` / `itemLimit` locals.
fn live_item_writes(build: &BuildRef) -> (pob_calc::lua_table::Table, pob_calc::lua_table::Table) {
    use pob_calc::lua_table::Table;
    let (radius, limit) = (Table::new(), Table::new());
    let main_env = build.calcsTab.mainEnv.get().unwrap();
    let items: Vec<_> = build
        .itemsTab
        .items
        .with(|m| m.iter().map(|(k, v)| (*k, v.clone())).collect());
    for (id, it) in items {
        if let Some(t) = it.jewelRadiusData.get() {
            radius.set(id as i64, Val::Table(t));
        }
    }
    let slots: Vec<_> = build.itemsTab.slots.with(|m| m.values().cloned().collect());
    for slot in slots {
        let node = slot.opt_num("nodeId");
        let Some(node) = node else { continue };
        if main_env
            .allocNodes
            .get()
            .with(|m| m.get(node as i64))
            .is_none()
        {
            continue;
        }
        let id = slot.num_or("selItemId", 0.0) as u32;
        let Some(it) = build.itemsTab.items.with(|m| m.get(&id).cloned()) else {
            continue;
        };
        if let Some(jd) = it.jewelData.get() {
            limit.set(id as i64, Val::Bool(jd.truthy("limitDisabled")));
        }
    }
    (radius, limit)
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

/// `xml` with a copy of its (single) `<Spec>` appended to `<Tree>`, without node `drop`.
fn with_second_spec(xml: &str, drop: f64) -> String {
    let start = xml.find("<Spec ").expect("<Spec");
    let end = xml[start..].find("</Spec>").expect("</Spec>") + start + "</Spec>".len();
    let spec = &xml[start..end];
    let a = spec.find(" nodes=\"").expect("nodes attribute") + " nodes=\"".len();
    let b = spec[a..].find('"').unwrap() + a;
    let drop = (drop as i64).to_string();
    let nodes: Vec<&str> = spec[a..b].split(',').filter(|n| *n != drop).collect();
    assert!(
        nodes.len() + 1 == spec[a..b].split(',').count(),
        "node {drop} in the spec"
    );
    let second = format!("{}{}{}", &spec[..a], nodes.join(","), &spec[b..]);
    let mut out = String::with_capacity(xml.len() + second.len());
    out.push_str(&xml[..end]);
    out.push_str("\n\t\t");
    out.push_str(&second);
    out.push_str(&xml[end..]);
    out
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
    // three fixtures (each case = two PoB boots, or two cache reads)
    let three: Vec<PathBuf> = fixtures().into_iter().step_by(2).collect();
    let results = pob_ui_oracle::oracle_cache::par_map(three, |path| {
        let mut fails = Vec::new();
        let mut lines = Vec::new();
        let mut cases_run = 0;
        let mut totals = (0usize, 0usize, 0usize, 0usize, 0usize);
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        let xml = std::fs::read_to_string(&path).unwrap();
        // a second, inactive tree spec without one allocated notable (TreeTab.lua:70's case)
        let (add, rem) = pick_nodes(&load(&xml));
        let xml = with_second_spec(&xml, rem);
        let build = load(&xml);
        assert_eq!(
            build.treeTab.specList.with(Vec::len),
            2,
            "{stem}: two specs"
        );
        let mut bridge = LuaCalcBridge::new(Some(src.clone()));
        assert!(matches!(bridge.build_output(&build), Poll::Ready(())));
        let (calc, _) = match bridge.misc_calculator(&build) {
            Poll::Ready(x) => x,
            Poll::Pending => unreachable!(),
        };
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
        let spec2 = build.treeTab.specList.with(|l| l[1].clone());
        cases.push((
            "spec (specList[2])".into(),
            Some(Rc::new({
                let o = Override::default();
                o.spec.set(Some(spec2));
                o
            })),
            None,
            "return { spec = b.treeTab.specList[2] }".into(),
        ));
        // ItemsTab.lua:4867 / :4903 (the flask / tincture tooltips), live items by identity
        for (kind, field) in [("Flask", "toggleFlask"), ("Tincture", "toggleTincture")] {
            let Some(it) = (1..=5)
                .filter_map(|i| slot_item(&build, &format!("Flask {i}")))
                .find(|it| it.r#type.get().as_str() == kind)
            else {
                continue;
            };
            let id = it.id.get().unwrap();
            let o = Override::default();
            match kind {
                "Flask" => o.toggleFlask.set(Some(it)),
                _ => o.toggleTincture.set(Some(it)),
            }
            cases.push((
                format!("{field} (item {id})"),
                Some(Rc::new(o)),
                None,
                format!("return {{ {field} = b.itemsTab.items[{id}] }}"),
            ));
        }
        // override.conditions (CalcSetup.lua:696-699), copied back after the call
        cases.push((
            "conditions {Onslaught, FullLife}".into(),
            Some(Rc::new({
                let o = Override::default();
                o.conditions.set(Some(Shared::new(vec![
                    pob_lua::Name::from("Onslaught"),
                    pob_lua::Name::from("FullLife"),
                ])));
                o
            })),
            None,
            "return { conditions = { \"Onslaught\", \"FullLife\" } }".into(),
        ));
        let mut none_out: Option<J> = None;
        for (name, ov, full, lua_body) in cases {
            let ov_after = ov.clone();
            let ours = call(&calc, ov, full);
            // through the Lua-side cache (TP2), as the bridge's own call
            let pob = LuaSession::cached_call_misc(
                &src,
                &reference_xml(&xml),
                "Unnamed build",
                &lua_body,
                full,
            )
            .expect("reference load and calcFunc");
            let pob_root = resolve_ids(&pob.root);
            let mut locals = vec![
                ("output".into(), Val::Table(ours)),
                ("socketGroupList".into(), groups_val(&build)),
            ];
            if let Some(c) = ov_after.as_ref().and_then(|o| o.conditions.get()) {
                let t = pob_calc::lua_table::Table::new();
                for n in c.snapshot() {
                    t.push(Val::Str(n.as_str().into()));
                }
                locals.push(("conditions".into(), Val::Table(t)));
            }
            let ours_root = resolve_ids(&pob_calc::probe::export_root(None, &locals));
            let mut g = diff_at(&pob_root, &ours_root, "locals.output");
            let gc = diff_at(&pob_root, &ours_root, "locals.conditions");
            g.hard.extend(gc.hard);
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
                || name.starts_with("spec")
                || name.contains("Ring")
            {
                assert!(
                    changed > 0,
                    "{stem} {name}: PoB's output equals the no-override output"
                );
            }
            lines.push(format!(
                "{stem} {name}: {leaves} leaves ({changed} differ from no override), {} mismatches (+{} order-only, {} ref-path-only, {} nil-default)",
                g.hard.len(),
                g.order_only,
                g.ref_only,
                g.nil_default
            ));
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
        (lines, fails, cases_run, totals)
    });
    let mut fails = Vec::new();
    let mut cases_run = 0;
    let mut totals = (0usize, 0usize, 0usize, 0usize, 0usize);
    for (lines, f, c, t) in results {
        for l in lines {
            println!("{l}");
        }
        fails.extend(f);
        cases_run += c;
        totals.0 += t.0;
        totals.1 += t.1;
        totals.2 += t.2;
        totals.3 += t.3;
        totals.4 += t.4;
    }
    println!("{}", pob_ui_oracle::oracle_cache::summary());
    println!(
        "TOTAL misc cases {cases_run}, leaves {}: {} mismatches, {} order-only, {} ref-path-only, {} nil-default",
        totals.0, totals.1, totals.2, totals.3, totals.4
    );
    assert!(fails.is_empty(), "mismatches:\n{}", fails.join("\n"));
}

/// The calc's writes into the model through the bridge (CalcSetup.lua:1556-1558, :1586, :1589):
/// a group whose source no item grants is removed and stops being `displayGroup`; out-of-range
/// `mainSocketGroup` / `skill_number` are clamped; a kept `displayGroup` stays.
#[test]
fn bridge_model_writes() {
    let Some(src) = inputs() else { return };
    // a heavy case on this thread: one of the process-wide pool permits (TP2)
    let _permit = pob_ui_oracle::oracle_cache::Permit::acquire();
    let path = &fixtures()[0];
    let build = load(&std::fs::read_to_string(path).unwrap());
    let n0 = build.skillsTab.socketGroupList.get().with(Vec::len);
    let orphan = Rc::new(pob_calc::model::SocketGroup::default());
    orphan.label.set(pob_calc::lua_table::LStr::from("orphan"));
    orphan.enabled.set(true);
    orphan.source.set(Some(pob_calc::lua_table::LStr::from(
        "Item:999999:No Such Item",
    )));
    build
        .skillsTab
        .socketGroupList
        .get()
        .with_mut(|v| v.push(orphan.clone()));
    build.skillsTab.displayGroup.set(Some(orphan.clone()));
    build.mainSocketGroup.set(99.0);
    build.calcsTab.input.set("skill_number", 99.0);
    let mut bridge = LuaCalcBridge::new(Some(src.clone()));
    assert!(matches!(bridge.build_output(&build), Poll::Ready(())));
    let groups = build.skillsTab.socketGroupList.get().snapshot();
    assert!(
        !groups.iter().any(|g| Rc::ptr_eq(g, &orphan)),
        "the calc removed the orphan group"
    );
    assert!(
        build.skillsTab.displayGroup.get().is_none(),
        "displayGroup cleared"
    );
    assert!(bridge.last_report.as_ref().unwrap().display_group_cleared);
    let n = groups.len() as f64;
    assert_eq!(
        build.mainSocketGroup.get(),
        n.max(1.0),
        "mainSocketGroup clamped"
    );
    assert_eq!(
        build.calcsTab.input.opt_num("skill_number"),
        Some(n.max(1.0)),
        "skill_number clamped"
    );
    println!("groups {n0} + orphan -> {n}; mainSocketGroup/skill_number 99 -> {n}");
    // a kept display group stays
    let first = groups[0].clone();
    build.skillsTab.displayGroup.set(Some(first.clone()));
    assert!(matches!(bridge.build_output(&build), Poll::Ready(())));
    assert!(build
        .skillsTab
        .displayGroup
        .get()
        .is_some_and(|d| Rc::ptr_eq(&d, &first)));
    assert!(!bridge.last_report.as_ref().unwrap().display_group_cleared);
}

#[test]
fn rust_calc_runs_on_stubs() {
    let Some(_) = inputs() else { return };
    // a heavy case on this thread: one of the process-wide pool permits (TP2)
    let _permit = pob_ui_oracle::oracle_cache::Permit::acquire();
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
/// gate's categories, to show which order-only differences are PoB's own (§9.5). Uncached (two
/// fresh VMs are the point).
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

/// `reference_xml` pins exactly the ambiguous old-format gems (the Vaal Righteous Fire of
/// `upstream_Dual_Wield_Cospris_CoC`) and leaves the other gems untouched.
#[test]
fn reference_xml_pins_ambiguous_gems() {
    let Some(_) = inputs() else { return };
    let mut pinned = 0;
    for path in fixtures() {
        let xml = std::fs::read_to_string(&path).unwrap();
        let r = reference_xml(&xml);
        let n = r.matches("<Gem variantId=").count();
        if path.to_string_lossy().contains("Cospris") {
            assert!(
                r.contains("<Gem variantId=\"VaalRighteousFire\" "),
                "Vaal RF pinned"
            );
        }
        assert_eq!(r.len() > xml.len(), n > 0);
        pinned += n;
    }
    assert!(pinned >= 1);
    println!("pinned {pinned} gems");
}
