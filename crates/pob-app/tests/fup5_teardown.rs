//! FUP5 (FOLLOWUPS "CCRDOC" 7, CCR-0142 §3 B): the desktop app frees a build it drops and a socket
//! group the Skills tab deletes. Calc doc §3.3 rule 4: `Env.build` / `MiscCalculator.build` ↔
//! `calcsTab.{mainEnv, calcsEnv, miscCalculator}` and `ActiveSkill.socketGroup` ↔
//! `SocketGroup.displaySkillList(Calcs)` are strong (Lua aliases), so whoever drops a build or a
//! socket group breaks them first (`Build::teardown`, `SocketGroup::teardown`).
//!
//! * `build_switch_frees_the_previous_build`: the real `PobApp` (Launch → Build mode, the Rust
//!   calc) opens a minion build, then another one (`build/init.rs` drops the first `BuildState`).
//!   A `Weak` to the first build's model and to every env, actor, skill and mod store reachable
//!   from its `mainEnv`, `calcsEnv` and the `GlobalCache` must be dead (before the fix the build
//!   and 100-252 objects survived, CCR-0142 §1).
//! * `group_delete_frees_its_display_skills`: the Skills tab's Delete button on a socket group
//!   with gems (its confirm popup, SkillListControl.lua:185-195), then the recalc `buildFlag`
//!   asks for: the group and its last display skills must be dead.
//!
//! The graph walk is `pob-calc/tests/leak1_cycles.rs`'s. Run with `--test-threads=1` (each test
//! holds a build or two in memory).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};

use pob_app::app::CxApp;
use pob_calc::active_skill::{ActiveSkill, ActiveSkillRef};
use pob_calc::actor::{Actor, ActorRef};
use pob_calc::classes::ModStore;
use pob_calc::env::{Env, EnvRef};
use pob_calc::global_cache::GlobalCache;
use pob_calc::model::{Build, SocketGroup};
use pob_host::App;
use pob_sg::Key;
use pob_ui_core::controls::list::{ListControl, ListValue};
use pob_ui_core::controls::ButtonControl;

fn root() -> Option<PathBuf> {
    let root = pob_testpaths::require(pob_testpaths::pob_root(), "PoB checkout (.PathOfBuilding)")?;
    let dump = pob_testpaths::require(pob_testpaths::dump_dir(), "data/dump")?;
    pob_gamedata::init_from_dir(&dump).expect("data dump");
    let src = std::fs::canonicalize(root.join("src")).unwrap_or_else(|_| root.join("src"));
    pob_gamedata::timeless_lut::set_jewel_fs(std::sync::Arc::new(
        pob_gamedata::timeless_lut::StdJewelFs {
            script_path: src.to_string_lossy().into_owned(),
            writable: false,
        },
    ));
    Some(root)
}

fn fixture_user() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../pob-ui-oracle/fixtures/user1")
}

fn fixture_xml(name: &str) -> String {
    let p = fixture_user().join("Path of Building/Builds").join(name);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

/// The headless app on the fixture user dir, past `OnInit` and its first frame.
struct Headless {
    app: pob_app::PobApp,
    h: pob_host::headless::Headless,
}

impl Headless {
    fn new(root: &Path) -> Headless {
        let src = std::fs::canonicalize(root.join("src")).unwrap();
        let runtime = std::fs::canonicalize(root.join("runtime")).unwrap();
        let state = pob_host::headless::HeadlessState::new(
            &src.to_string_lossy(),
            &runtime.to_string_lossy(),
        );
        *state.user_path.borrow_mut() = format!("{}/", fixture_user().display());
        let (platform, fs, net) = pob_host::headless::services(&state);
        let mut parts = pob_host::headless::parts(Rc::new(pob_host::fs_native::DirAssets {
            src: src.clone(),
            runtime: runtime.join("SimpleGraphic"),
        }))
        .expect("fonts");
        parts.sys = pob_sg::Sys::new(platform, fs, net);
        let mut app = pob_app::PobApp::new(parts);
        // the fixture's manifest-less checkout would turn dev mode on (CCR-0096)
        app.cx.app_mut().launch.force_release_install = true;
        let mut s = Headless { app, h: state };
        pob_host::guarded(&mut s.app, App::on_init).expect("on_init");
        s.frame();
        s
    }

    fn frame(&mut self) {
        let screen = pob_sg::ScreenInfo {
            fb_w: 1280,
            fb_h: 800,
            win_h: 800,
            dpi_scale: 1.0,
        };
        pob_host::run_frame(&mut self.app, screen, (0, 0)).expect("frame");
        self.h.clock_ms.set(self.h.clock_ms.get() + 16);
    }

    fn open(&mut self, name: &str) {
        let xml = fixture_xml(&format!("{name}.xml"));
        pob_host::guarded(&mut self.app, |a| a.load_build(name, &xml)).expect("load");
        for _ in 0..3 {
            self.frame();
        }
        assert_eq!(
            self.app.cx.app().main.mode.as_deref(),
            Some("BUILD"),
            "{name}: in Build mode"
        );
    }
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

    fn len(&self) -> usize {
        self.envs.len() + self.actors.len() + self.skills.len() + self.stores.len()
    }

    /// Labels of every object that is still alive.
    fn survivors(&self) -> Vec<String> {
        fn alive<T>(v: &[(String, Weak<T>)], kind: &str) -> Vec<String> {
            v.iter()
                .filter(|(_, w)| w.strong_count() > 0)
                .map(|(l, w)| format!("{kind} {l} (strong {})", w.strong_count()))
                .collect()
        }
        let mut v = alive(&self.envs, "env");
        v.extend(alive(&self.actors, "actor"));
        v.extend(alive(&self.skills, "skill"));
        v.extend(alive(&self.stores, "store"));
        v
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

/// Every object the build's calc reached: its `mainEnv`, `calcsEnv` and the `GlobalCache`.
fn build_graph(build: &Build) -> Graph {
    let mut g = Graph::default();
    if let Some(e) = build.calcsTab.mainEnv.get() {
        g.env("mainEnv", &e);
    }
    if let Some(e) = build.calcsTab.calcsEnv.get() {
        g.env("calcsEnv", &e);
    }
    cached_envs(&mut g);
    g
}

#[test]
fn build_switch_frees_the_previous_build() {
    let Some(root) = root() else { return };
    let mut s = Headless::new(&root);
    // allflame_exp_001: a minion build (CCR-0142 §1: 136 objects survived the drop)
    s.open("allflame_exp_001");
    let (weak_build, g) = {
        let model = s.app.cx.build().model.clone();
        (Rc::downgrade(&model), build_graph(&model))
    };
    assert!(
        g.envs.len() >= 2 && !g.skills.is_empty(),
        "graph: {} objects",
        g.len()
    );
    let groups: Vec<Weak<SocketGroup>> = weak_build
        .upgrade()
        .expect("model")
        .skillsTab
        .socketGroupList
        .get()
        .snapshot()
        .iter()
        .map(Rc::downgrade)
        .collect();
    assert!(!groups.is_empty());

    // another build: `build/init.rs` drops the first BuildState
    s.open("allflame_exp_007");
    s.frame();
    let mut survivors = g.survivors();
    let live_groups = groups.iter().filter(|w| w.strong_count() > 0).count();
    eprintln!(
        "build switch: first build strong {}; {} of {} objects and {live_groups} of {} socket groups survive",
        weak_build.strong_count(),
        survivors.len(),
        g.len(),
        groups.len()
    );
    if weak_build.strong_count() > 0 {
        survivors.insert(0, format!("build (strong {})", weak_build.strong_count()));
    }
    if live_groups > 0 {
        survivors.push(format!("{live_groups} socket groups"));
    }
    assert!(
        survivors.is_empty(),
        "the dropped build kept alive:\n  {}",
        survivors.join("\n  ")
    );
}

#[test]
fn group_delete_frees_its_display_skills() {
    let Some(root) = root() else { return };
    let mut s = Headless::new(&root);
    s.open("allflame_exp_001");
    let cx = &mut s.app.cx;
    let model = cx.build().model.clone();
    // the first group a user can delete that has display skills (no `source`, gems)
    let list = model.skillsTab.socketGroupList.get();
    let (index, group) = list
        .snapshot()
        .into_iter()
        .enumerate()
        .find(|(_, g)| {
            g.source.get().is_none()
                && !g.gemList.get().is_empty()
                && g.displaySkillList.get().is_some_and(|l| !l.is_empty())
        })
        .expect("a deletable group with display skills");
    let index = index + 1;
    let skills: Vec<(String, Weak<ActiveSkill>)> = group
        .displaySkillList
        .get()
        .expect("checked")
        .snapshot()
        .iter()
        .chain(
            group
                .displaySkillListCalcs
                .get()
                .map(|l| l.snapshot())
                .unwrap_or_default()
                .iter(),
        )
        .map(|s| (skill_name(s), Rc::downgrade(s)))
        .collect();
    let weak_group = Rc::downgrade(&group);
    drop(group);
    drop(list);
    drop(model);

    // the Skills tab's group list: select the row, click Delete (SkillListControl.lua:38-41)
    let skills_tab = cx.build().tabs.skills.id();
    let gl = cx.child(skills_tab, "groupList").expect("groupList");
    {
        let g = weak_group.upgrade().expect("group");
        let w = cx.w_mut::<ListControl>(gl);
        w.sel_index = Some(index);
        w.sel_value = Some(ListValue::of(g));
    }
    let del = cx.child(gl, "delete").expect("delete button");
    let on_click = cx.w::<ButtonControl>(del).on_click.clone();
    on_click(cx);
    assert!(!cx.main.popups.is_empty(), "the confirm popup is open");
    // RETURN clicks the popup's enterControl, "confirm" (Main.lua OpenConfirmPopup)
    s.app.on_key_down(Key::Return, false);
    s.app.on_key_up(Key::Return);
    // the confirm runs, then the recalc buildFlag asks for (Build.lua:1258-1266)
    for _ in 0..3 {
        s.frame();
    }
    let cx = &s.app.cx;
    assert!(cx.main.popups.is_empty(), "the popup closed");
    assert!(
        !cx.build()
            .model
            .skillsTab
            .socketGroupList
            .get()
            .snapshot()
            .iter()
            .any(|g| weak_group.upgrade().is_some_and(|w| Rc::ptr_eq(&w, g))),
        "the group left socketGroupList"
    );
    let mut survivors: Vec<String> = skills
        .iter()
        .filter(|(_, w)| w.strong_count() > 0)
        .map(|(l, w)| format!("display skill {l} (strong {})", w.strong_count()))
        .collect();
    eprintln!(
        "group delete: group strong {}; {} of {} display skills survive",
        weak_group.strong_count(),
        survivors.len(),
        skills.len()
    );
    if weak_group.strong_count() > 0 {
        survivors.insert(0, format!("group (strong {})", weak_group.strong_count()));
    }
    assert!(
        survivors.is_empty(),
        "the deleted group kept alive:\n  {}",
        survivors.join("\n  ")
    );
}
