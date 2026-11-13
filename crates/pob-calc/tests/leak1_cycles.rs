//! LEAK1 (CCR-0135): no `Rc` cycle keeps a calc env's graph alive once its owners are gone
//! (calc doc §3.3 rule 4: back-pointers are `Weak`, `Env` owns its actors, `GlobalCache` owns
//! cached envs and wiping it frees them).
//!
//! The CCR's diagnosis, as a test: take a `Weak` to every env, actor, active skill and actor
//! mod store reachable from an env (the player, the enemy, every skill's minion and the
//! minions' skills), drop the owners, wipe the `GlobalCache`, then every upgrade must fail.
//!
//! * `calculator_env_frees_its_graph`: `initEnv` + `perform` in `Mode::CALCULATOR` (the misc
//!   calculator's env of the power report), then drop the env.
//! * `recalc_frees_the_previous_graphs`: a loaded build's `mainEnv`, `calcsEnv` and every
//!   `GlobalCache` entry's env (`cacheData`), then one recalc (`wipeGlobalCache` +
//!   `CalcsTab:BuildOutput`, Build.lua:1258-1266), which replaces all of them.
//! * `build_teardown_frees_everything_ccr0142`: dropping a build after breaking the two
//!   build-level cycles of CCR-0142 frees it and its envs.
//!
//! Builds: the three minion builds of the CCR (allflame_exp_039, _007, _001) and one without a
//! minion skill (allflame_exp_024; in the quick corpus, unlike _000). Before CCR-0135 every skill with a `minion` survived.
#![allow(non_snake_case)]

use std::collections::HashSet;
use std::rc::{Rc, Weak};
use std::sync::Arc;

use pob_calc::active_skill::{ActiveSkill, ActiveSkillRef};
use pob_calc::actor::{Actor, ActorRef};
use pob_calc::classes::ModStore;
use pob_calc::env::{Env, EnvRef, Mode};
use pob_calc::global_cache::GlobalCache;
use pob_calc::model::BuildRef;
use pob_gamedata::timeless_lut as lut;

/// (build, has a minion skill).
const BUILDS: [(&str, bool); 4] = [
    ("allflame_exp_039", true),
    ("allflame_exp_007", true),
    ("allflame_exp_001", true),
    ("allflame_exp_024", false),
];

fn init() -> bool {
    static INIT: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *INIT.get_or_init(|| {
        let Some(dump) = pob_testpaths::require(pob_testpaths::dump_dir(), "data/dump") else {
            return false;
        };
        pob_gamedata::init_from_dir(&dump).expect("data.bin");
        if let Some(src) = pob_testpaths::pob_src() {
            let src = std::fs::canonicalize(&src).unwrap_or(src);
            lut::set_jewel_fs(Arc::new(lut::StdJewelFs {
                script_path: src.to_string_lossy().into_owned(),
                writable: false,
            }));
            lut::set_con_printf(|_| {});
        }
        true
    })
}

fn build_xml(stem: &str) -> Option<String> {
    let p = pob_testpaths::require(
        pob_testpaths::parity_build(stem),
        &format!(".parity/builds/{stem}.xml"),
    )?;
    Some(std::fs::read_to_string(p).expect("read build"))
}

/// `Weak`s to everything reachable from the envs given (deduplicated by address).
#[derive(Default)]
struct Graph {
    seen: HashSet<usize>,
    envs: Vec<(String, Weak<Env>)>,
    actors: Vec<(String, Weak<Actor>)>,
    skills: Vec<(String, Weak<ActiveSkill>)>,
    stores: Vec<(String, Weak<ModStore>)>,
}

fn skill_name(s: &ActiveSkill) -> String {
    s.activeEffect.get().grantedEffect.get().name.to_string()
}

impl Graph {
    fn first<T>(&mut self, r: &Rc<T>) -> bool {
        self.seen.insert(Rc::as_ptr(r) as *const () as usize)
    }

    fn env(&mut self, label: &str, env: &EnvRef) {
        if !self.first(env) {
            return;
        }
        self.envs.push((label.to_owned(), Rc::downgrade(env)));
        self.actor(&format!("{label}.player"), &env.player);
        self.actor(&format!("{label}.enemy"), &env.enemy);
        if let Some(m) = env.minion.get() {
            self.actor(&format!("{label}.minion"), &m);
        }
        // `env.partyMembers = env.build.partyTab.actor` (CalcPerform.lua:1074): the build's
        // party actor, shared by every env of the build and owned by the build, not the env.
        for s in env.auxSkillList.get().snapshot() {
            self.skill(&format!("{label}.auxSkillList"), &s);
        }
    }

    fn actor(&mut self, label: &str, a: &ActorRef) {
        if !self.first(a) {
            return;
        }
        self.actors.push((label.to_owned(), Rc::downgrade(a)));
        let db = a.modDB.get();
        if self.first(&db) {
            self.stores
                .push((format!("{label}.modDB"), Rc::downgrade(&db)));
        }
        for s in a.activeSkillList.get().snapshot() {
            self.skill(&format!("{label}.activeSkillList"), &s);
        }
        if let Some(s) = a.mainSkill.get() {
            self.skill(&format!("{label}.mainSkill"), &s);
        }
        if let Some(m) = a.minion.get() {
            self.actor(&format!("{label}.minion"), &m);
        }
    }

    fn skill(&mut self, label: &str, s: &ActiveSkillRef) {
        if !self.first(s) {
            return;
        }
        let label = format!("{label}[{}]", skill_name(s));
        self.skills.push((label.clone(), Rc::downgrade(s)));
        for (k, l) in [
            ("skillModList", s.skillModList.get()),
            ("baseSkillModList", s.baseSkillModList.get()),
        ] {
            if self.first(&l) {
                self.stores
                    .push((format!("{label}.{k}"), Rc::downgrade(&l)));
            }
        }
        if let Some(m) = s.minion.get() {
            self.actor(&format!("{label}.minion"), &m);
        }
    }

    /// Labels of every object that is still alive.
    fn survivors(&self) -> Vec<String> {
        let mut v = Vec::new();
        v.extend(
            self.envs
                .iter()
                .filter(|(_, w)| w.strong_count() > 0)
                .map(|(l, w)| format!("env {l} (strong {})", w.strong_count())),
        );
        v.extend(
            self.actors
                .iter()
                .filter(|(_, w)| w.strong_count() > 0)
                .map(|(l, w)| format!("actor {l} (strong {})", w.strong_count())),
        );
        v.extend(
            self.skills
                .iter()
                .filter(|(_, w)| w.strong_count() > 0)
                .map(|(l, w)| format!("skill {l} (strong {})", w.strong_count())),
        );
        v.extend(
            self.stores
                .iter()
                .filter(|(_, w)| w.strong_count() > 0)
                .map(|(l, w)| format!("store {l} (strong {})", w.strong_count())),
        );
        v
    }

    /// Live minion skills whose `summonSkill` is set (the back-pointer CCR-0135 made `Weak`).
    fn with_summon_skill(&self) -> usize {
        self.skills
            .iter()
            .filter(|(_, w)| w.upgrade().is_some_and(|s| s.summonSkill.get().is_some()))
            .count()
    }

    fn summary(&self) -> String {
        format!(
            "{} envs, {} actors ({} minions), {} skills ({} with a minion, {} with a summonSkill), {} stores",
            self.envs.len(),
            self.actors.len(),
            self.actors.iter().filter(|(l, _)| l.ends_with(".minion")).count(),
            self.skills.len(),
            self.skills.iter().filter(|(_, w)| w.upgrade().is_some_and(|s| s.minion.get().is_some())).count(),
            self.with_summon_skill(),
            self.stores.len(),
        )
    }
}

fn cached_envs(g: &mut Graph) {
    let entries: Vec<_> = GlobalCache.with(|c| {
        c.borrow()
            .cachedData
            .iter()
            .flat_map(|m| m.values().cloned().collect::<Vec<_>>())
            .collect()
    });
    for e in entries {
        g.env(&format!("GlobalCache[{}]", e.Name), &e.Env);
        g.skill(
            &format!("GlobalCache[{}].ActiveSkill", e.Name),
            &e.ActiveSkill,
        );
    }
}

#[test]
fn calculator_env_frees_its_graph() {
    if !init() {
        return; // POB_ALLOW_SKIP=1
    }
    let mut failures = Vec::new();
    for (stem, minions) in BUILDS {
        let Some(xml) = build_xml(stem) else { return };
        let build: BuildRef =
            pob_calc::model::build_xml::loadBuildModelFromXML(&xml, pob_gamedata::data())
                .expect("load");
        pob_calc::modules::common::wipeGlobalCache();
        let mut g = Graph::default();
        {
            let (env, _, _, _) =
                pob_calc::modules::calc_setup::initEnv(&build, Mode::CALCULATOR, None, None);
            pob_calc::modules::calc_perform::perform(&env, false);
            g.env("env", &env);
            cached_envs(&mut g);
        }
        // The walk reaches the minion skills (the case CCR-0135 is about).
        assert_eq!(
            g.with_summon_skill() > 0,
            minions,
            "{stem}: minion skills with a summonSkill: {}",
            g.with_summon_skill()
        );
        let summary = g.summary();
        pob_calc::modules::common::wipeGlobalCache();
        let s = g.survivors();
        eprintln!("{stem}: CALCULATOR env: {summary}; {} survive", s.len());
        if !s.is_empty() {
            failures.push(format!("{stem}:\n  {}", s.join("\n  ")));
        }
        drop(build);
    }
    assert!(
        failures.is_empty(),
        "objects kept alive after the env was dropped and the GlobalCache wiped:\n{}",
        failures.join("\n")
    );
}

#[test]
fn recalc_frees_the_previous_graphs() {
    if !init() {
        return; // POB_ALLOW_SKIP=1
    }
    let mut failures = Vec::new();
    for (stem, minions) in BUILDS {
        let Some(xml) = build_xml(stem) else { return };
        pob_calc::global_cache::clear_all();
        // loadBuildFromXML runs the first BuildOutput.
        let build = pob_calc::model::build_xml::loadBuildFromXML(&xml, pob_gamedata::data());
        let mut g = Graph::default();
        if let Some(e) = build.calcsTab.mainEnv.get() {
            g.env("mainEnv", &e);
        }
        if let Some(e) = build.calcsTab.calcsEnv.get() {
            g.env("calcsEnv", &e);
        }
        cached_envs(&mut g);
        assert!(!g.envs.is_empty(), "{stem}: no env after the load");
        assert_eq!(
            g.with_summon_skill() > 0,
            minions,
            "{stem}: minion skills with a summonSkill: {}",
            g.with_summon_skill()
        );
        let summary = g.summary();
        // Build.lua:1260 wipeGlobalCache(), 1266 self.calcsTab:BuildOutput()
        pob_calc::modules::common::wipeGlobalCache();
        pob_calc::modules::power::BuildOutput(&build);
        let s = g.survivors();
        eprintln!(
            "{stem}: before the recalc: {summary}; {} survive the recalc",
            s.len()
        );
        if !s.is_empty() {
            failures.push(format!("{stem}:\n  {}", s.join("\n  ")));
        }
        drop(build);
        pob_calc::global_cache::clear_all();
    }
    assert!(
        failures.is_empty(),
        "objects of the previous calc kept alive by a recalc:\n{}",
        failures.join("\n")
    );
}

/// CCR-0142: the two build-level cycles, which keep a dropped build alive (not a recalc;
/// `recalc_frees_the_previous_graphs` passes). Four variants per build: (a) drop the build as it
/// is; (b) first clear `calcsTab.mainEnv/calcsEnv/miscCalculator` (the `Env.build` /
/// `MiscCalculator.build` edges back to the build); (c) also clear every socket group's
/// `displaySkillList(Calcs)` (the `ActiveSkill.socketGroup` edge back to the group); (d)
/// `Build::teardown` (CCR-0142 §3 B), which the web calc worker runs after each request
/// (`pob-app/src/calc/worker_web.rs` `run_request`) and the desktop app before it drops a build
/// (`pob-app/src/build/init.rs`). (a) and (b) are printed (they leak: the cycles are Lua aliases);
/// the gates are (c) and (d): the build and everything reachable from its envs is freed.
#[test]
fn build_teardown_frees_everything_ccr0142() {
    if !init() {
        return; // POB_ALLOW_SKIP=1
    }
    let mut failures = Vec::new();
    for (stem, _) in BUILDS {
        let Some(xml) = build_xml(stem) else { return };
        for variant in ['a', 'b', 'c', 'd'] {
            pob_calc::global_cache::clear_all();
            let build = pob_calc::model::build_xml::loadBuildFromXML(&xml, pob_gamedata::data());
            let mut g = Graph::default();
            if let Some(e) = build.calcsTab.mainEnv.get() {
                g.env("mainEnv", &e);
            }
            if let Some(e) = build.calcsTab.calcsEnv.get() {
                g.env("calcsEnv", &e);
            }
            let wb = Rc::downgrade(&build);
            if ('b'..='c').contains(&variant) {
                build.calcsTab.mainEnv.set(None);
                build.calcsTab.calcsEnv.set(None);
                build.calcsTab.miscCalculator.set(None);
            }
            if variant == 'd' {
                build.teardown();
            } else if variant == 'c' {
                for sg in build.skillsTab.socketGroupList.get().snapshot() {
                    sg.displaySkillList.set(None);
                    sg.displaySkillListCalcs.set(None);
                }
            }
            drop(build);
            pob_calc::modules::common::wipeGlobalCache();
            let s = g.survivors();
            let n = g.envs.len() + g.actors.len() + g.skills.len() + g.stores.len();
            eprintln!(
                "{stem} ({variant}): build strong {}; {} of {n} objects survive",
                wb.strong_count(),
                s.len()
            );
            if variant >= 'c' && (wb.strong_count() > 0 || !s.is_empty()) {
                failures.push(format!(
                    "{stem} ({variant}): build strong {}\n  {}",
                    wb.strong_count(),
                    s.join("\n  ")
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "the teardown left objects alive:\n{}",
        failures.join("\n")
    );
}
