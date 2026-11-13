//! @pob-file Classes/CalcsTab.lua @16de4b82d
//! @pob-file Modules/Data.lua @16de4b82d
//! CalcsTab.lua 441-773: BuildOutput, the power report (PowerBuilder as a resumable iterator),
//! CalculatePowerStat, CalculateCombinedOffDefStat, GetMiscCalculator, the undo state; and
//! `data.powerStatList.GetFromOutput` with its `transform` closures (Data.lua 131, 164-168,
//! 186-212). Sequential by default (D12).
//!
//! Owner: PWR. Representation (CCR-0101, `docs/port/ccr/0101-pwr-power-seams.md`):
//!
//! * (§1) The `calcsTab` fields the frozen `CalcsTab` struct has no slot for live in the build's
//!   side table (`build_xml::build_extra(build).calcsTab`) under their Lua names: `powerStat`
//!   (a `Table`: a `data.powerStatList` entry as [`powerStat_table`] builds it, or any
//!   `{ stat = … }` table), `powerMax` (a `Table`), `nodePowerMaxDepth`, `powerBuilder`,
//!   `powerBuilderInitialized`, and `miscCalculatorBase` (the second element of
//!   `self.miscCalculator`; the first is `calcsTab.miscCalculator`). The build's
//!   `powerBuilderCallback` / `powerBuilderProgressCallback` are in the build side table.
//!   Typed accessors: [`powerStat`], [`set_powerStat`], [`powerMax`], [`nodePowerMaxDepth`], …
//! * (§2) `CalculateCombinedOffDefStat` returns both Lua results `(offence, defence)`.
//! * (§3) The coroutine: [`PowerBuilder`] is one `coroutine.resume` (it returns `Some(())` when
//!   the body yielded, `None` when it finished). `coroutine.running()` is true inside it;
//!   [`PowerBuilder_run`] is the body called directly (no coroutine, never yields), as Lua's
//!   `calcsTab:PowerBuilder()` outside a coroutine. `GetTime()` is [`GetTime`] (a host hook,
//!   [`set_GetTime`]); `launch.devMode` is [`launch_devMode`] (the UI sets it).
//! * (§4) Parallel mode (`PowerOptions { parallel: true }`, D12, opt-in): the step that leaves
//!   the first yield records the body's calc calls in order (a dry run on placeholder outputs:
//!   which calls the body makes depends only on the tree, the cache keys and `powerStat`), runs
//!   them on worker threads, each with its own `Build` loaded from `build:SaveDB` XML (one
//!   GlobalCache per worker), then runs the body on those results. Results may differ from the
//!   sequential run where the calc reads GlobalCache entries the sequential order left behind
//!   (mirage / trigger skills, D11); `PWR.rs` reports those differences.
#![allow(non_snake_case, clippy::too_many_lines, clippy::float_cmp)]

use std::cell::RefCell;

use pob_lua::pattern::gsub;

use crate::classes::passive_tree::new_node_table;
use crate::model::build_xml::build_extra;
use crate::modules::calcs::{getMiscCalculator, MiscCalculator};
use crate::prelude::*;

/// Power report options: `parallel` builds one Build per worker from the XML text (opt-in, D12).
#[derive(Default, Clone, Copy)]
pub struct PowerOptions {
    pub parallel: bool,
}

/// The PowerBuilder coroutine's state between resumes.
#[derive(Default)]
pub struct PowerState {
    pub done: bool,
    pub extra: Table,
}

// ---------------------------------------------------------------------------------------
// Port plumbing (snake_case): side-table fields, host hooks, callbacks (CCR-0101 §1, §3)
// ---------------------------------------------------------------------------------------

/// `build.calcsTab`'s side table (CCR-0101 §1).
fn ct_extra(build: &BuildRef) -> Table {
    build_extra(build).get_or_new_table(n!("calcsTab"))
}

/// `self.powerStat`: the selected `data.powerStatList` entry (`nil` = Offence/Defence).
pub fn powerStat(build: &BuildRef) -> Option<Table> {
    ct_extra(build).table(n!("powerStat"))
}

/// `calcsTab.powerStat = stat` (TreeTab:SetPowerCalc, U26).
pub fn set_powerStat(build: &BuildRef, stat: Option<Table>) {
    ct_extra(build).set(n!("powerStat"), stat);
}

/// `self.powerMax` (`nil` until the first `BuildPower` runs).
pub fn powerMax(build: &BuildRef) -> Option<Table> {
    ct_extra(build).table(n!("powerMax"))
}

/// `self.nodePowerMaxDepth` (`nil` = all nodes).
pub fn nodePowerMaxDepth(build: &BuildRef) -> Option<f64> {
    ct_extra(build).get(n!("nodePowerMaxDepth")).opt_num()
}

/// `calcsTab.nodePowerMaxDepth = depth` (TreeTab, U26).
pub fn set_nodePowerMaxDepth(build: &BuildRef, depth: Option<f64>) {
    ct_extra(build).set(n!("nodePowerMaxDepth"), depth);
}

/// `self.powerBuilderInitialized`.
pub fn powerBuilderInitialized(build: &BuildRef) -> bool {
    ct_extra(build).truthy(n!("powerBuilderInitialized"))
}

/// `self.powerBuilder ~= nil` (a power report is running).
pub fn powerBuilderRunning(build: &BuildRef) -> bool {
    ct_extra(build).truthy(n!("powerBuilder"))
}

/// `self.miscCalculator = { calcFunc, calcBase }` (CalcsTab.lua:471; CCR-0101 §1).
pub fn set_miscCalculator(build: &BuildRef, calc: Option<(Rc<MiscCalculator>, Output)>) {
    match calc {
        Some((f, base)) => {
            build.calcsTab.miscCalculator.set(Some(f));
            ct_extra(build).set(n!("miscCalculatorBase"), base);
        }
        None => {
            build.calcsTab.miscCalculator.set(None);
            ct_extra(build).set(n!("miscCalculatorBase"), Val::Nil);
        }
    }
}

/// Options [`BuildPower`] starts the report with (CCR-0101 §4; sequential by default).
pub fn set_powerOptions(build: &BuildRef, options: PowerOptions) {
    ct_extra(build).set(n!("powerOptionsParallel"), options.parallel);
}

fn powerOptions(build: &BuildRef) -> PowerOptions {
    PowerOptions {
        parallel: ct_extra(build).truthy(n!("powerOptionsParallel")),
    }
}

struct Callback(Rc<dyn Fn()>);
struct ProgressCallback(Rc<dyn Fn(f64)>);

/// `build.powerBuilderCallback = f` (Build.lua:21; TreeTab sets it, U26).
pub fn set_powerBuilderCallback(build: &BuildRef, f: Option<Rc<dyn Fn()>>) {
    build_extra(build).set(
        n!("powerBuilderCallback"),
        f.map_or(Val::Nil, |f| Val::Obj(Rc::new(Callback(f)))),
    );
}

/// `build.powerBuilderProgressCallback = f` (Build.lua:20; TreeTab sets it, U26).
pub fn set_powerBuilderProgressCallback(build: &BuildRef, f: Option<Rc<dyn Fn(f64)>>) {
    build_extra(build).set(
        n!("powerBuilderProgressCallback"),
        f.map_or(Val::Nil, |f| Val::Obj(Rc::new(ProgressCallback(f)))),
    );
}

fn powerBuilderCallback(build: &BuildRef) -> Option<Rc<dyn Fn()>> {
    build_extra(build)
        .obj::<Callback>(n!("powerBuilderCallback"))
        .map(|c| c.0.clone())
}

fn powerBuilderProgressCallback(build: &BuildRef) -> Option<Rc<dyn Fn(f64)>> {
    build_extra(build)
        .obj::<ProgressCallback>(n!("powerBuilderProgressCallback"))
        .map(|c| c.0.clone())
}

thread_local! {
    static DEV_MODE: Cell<bool> = const { Cell::new(false) };
    static GET_TIME: Cell<Option<fn() -> f64>> = const { Cell::new(None) };
    #[cfg(not(target_arch = "wasm32"))]
    static EPOCH: std::time::Instant = std::time::Instant::now();
}

/// `launch.devMode` (Launch.lua; the UI sets it, default false as in a release PoB).
pub fn launch_devMode() -> bool {
    DEV_MODE.with(Cell::get)
}

/// Set `launch.devMode` (U16).
pub fn set_launch_devMode(v: bool) {
    DEV_MODE.with(|c| c.set(v));
}

/// SimpleGraphic's `GetTime()`: milliseconds. Desktop default: since the thread's first call;
/// wasm default: `0` (the report then never yields on time; the host sets a clock).
pub fn GetTime() -> f64 {
    if let Some(f) = GET_TIME.with(Cell::get) {
        return f();
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        EPOCH.with(|e| e.elapsed().as_secs_f64() * 1000.0)
    }
    #[cfg(target_arch = "wasm32")]
    {
        0.0
    }
}

/// Replace the `GetTime()` clock (`None` = the default).
pub fn set_GetTime(f: Option<fn() -> f64>) {
    GET_TIME.with(|c| c.set(f));
}

/// A `data.powerStatList` entry as the Lua table callers pass around (`stat`, `label`,
/// `transform` as `Val::Obj(Rc<LuaFnRef>)` (as ItemsTab.lua:1309 stores it), the flags).
pub fn powerStat_table(entry: &pob_gamedata::schema::item_data::PowerStat) -> Table {
    let t = Table::new();
    t.set(n!("stat"), entry.stat.map_or(Val::Nil, Val::from));
    t.set(n!("label"), &*entry.label);
    t.set(
        n!("transform"),
        entry
            .transform
            .clone()
            .map_or(Val::Nil, |f| Val::Obj(Rc::new(f))),
    );
    for (k, v) in [
        (n!("combinedOffDef"), entry.combinedOffDef),
        (n!("ignoreForItems"), entry.ignoreForItems),
        (n!("ignoreForNodes"), entry.ignoreForNodes),
        (n!("reverseSort"), entry.reverseSort),
    ] {
        if v {
            t.set(k, true);
        }
    }
    t.set(n!("itemField"), entry.itemField.map_or(Val::Nil, Val::from));
    t
}

/// The first `data.powerStatList` entry whose `stat` is `stat`, as [`powerStat_table`].
pub fn powerStat_by_stat(stat: &str) -> Option<Table> {
    data()
        .powerStatList
        .stats
        .iter()
        .find(|e| e.stat.as_deref() == Some(stat))
        .map(powerStat_table)
}

/// Calc test seam (feature `probe`; CCR-0101 §5): answers `calcFunc(override, useFullDPS)` for the
/// oracle's L5 replay of PoB's calc results. Unset (the product), the real calculator runs.
#[cfg(feature = "probe")]
pub mod calc_replay {
    use super::*;

    pub type Hook = Rc<dyn Fn(&Override, Option<bool>) -> Option<Output>>;

    thread_local! {
        static HOOK: RefCell<Option<Hook>> = const { RefCell::new(None) };
    }

    /// Install (or clear) the hook.
    pub fn set(hook: Option<Hook>) {
        HOOK.with(|h| *h.borrow_mut() = hook);
    }

    pub(super) fn replay(ov: &Override, useFullDPS: Option<bool>) -> Option<Output> {
        let hook = HOOK.with(|h| h.borrow().clone())?;
        hook(ov, useFullDPS)
    }
}

// ---------------------------------------------------------------------------------------
// BuildOutput / BuildPower
// ---------------------------------------------------------------------------------------

/// @pob Classes/CalcsTab.lua:441-472 h=eb7f2e5e
/// `CalcsTab:BuildOutput()`.
pub fn BuildOutput(build: &BuildRef) {
    // L442
    build.calcsTab.powerBuildFlag.set(true);

    // L444-452 (commented-out profiling block)

    // L454 LUA-ORDER: pairs(spec.nodes) (each node is written once; order is not observable)
    let spec = build.spec.get();
    for (_, node) in spec.nodes.with(LjIntMap::pairs) {
        // Set default final mod list for all nodes; some may not be set during the main pass
        // L456
        node.finalModList.set(Some(node.modList.get()));
    }

    // L459
    let mainEnv = crate::modules::calcs::buildOutput(build, Mode::MAIN);
    build.calcsTab.mainEnv.set(Some(mainEnv.clone()));
    // L460
    build
        .calcsTab
        .mainOutput
        .set(Some(mainEnv.player.output.get()));
    // L461
    let calcsEnv = crate::modules::calcs::buildOutput(build, Mode::CALCS);
    build.calcsTab.calcsEnv.set(Some(calcsEnv.clone()));
    // L462
    build
        .calcsTab
        .calcsOutput
        .set(Some(calcsEnv.player.output.get()));

    // L464-467 UI: if self.displayData then self.controls.breakdown:SetBreakdownData(…) end
    // (the breakdown control is U39/U40's; it re-reads its data on draw)

    // Retrieve calculator functions
    // L470
    let (miscCalcFunc, miscCalcBase) = getMiscCalculator(build);
    // L471
    set_miscCalculator(build, Some((miscCalcFunc, miscCalcBase)));
}

/// @pob Classes/CalcsTab.lua:475-493 h=5c678572
/// `CalcsTab:BuildPower()`: starts the power report (when `BuildOutput` flagged it) and resumes it
/// once. Called once per frame where PoB calls it (PassiveTreeView.lua:785, U26).
pub fn BuildPower(build: &BuildRef) {
    let ct = ct_extra(build);
    // L476
    if build.calcsTab.powerBuildFlag.get() {
        // L477
        build.calcsTab.powerBuildFlag.set(false);
        // L478
        ct.set(n!("powerMax"), Val::Nil);
        // L479
        let state: Rc<RefCell<PowerState>> = Rc::new(RefCell::new(PowerState::default()));
        ct.set(n!("powerBuilder"), Val::Obj(state));
    }
    // L481
    let Some(powerBuilder) = ct.obj::<RefCell<PowerState>>(n!("powerBuilder")) else {
        return;
    };
    // L482 coroutine.resume(self.powerBuilder, self)
    let options = powerOptions(build);
    // resuming the running coroutine (BuildPower from a progress callback) returns
    // `false, "cannot resume non-suspended coroutine"`; its status stays "running" (not dead)
    let (res, dead): (Result<(), String>, bool) = match powerBuilder.try_borrow_mut() {
        Ok(mut state) => {
            let r = resume(&mut state, build, &options);
            (r, state.done)
        }
        Err(_) => (
            Err("cannot resume non-suspended coroutine".to_owned()),
            false,
        ),
    };
    // L483
    if launch_devMode() {
        if let Err(errMsg) = &res {
            // L484
            lua_error!("{errMsg}")
        }
    }
    // L486 `coroutine.status(self.powerBuilder)` re-reads the field: a callback may have started
    // (or, nested, finished) another report during the resume
    let dead = match ct.obj::<RefCell<PowerState>>(n!("powerBuilder")) {
        Some(p) if Rc::ptr_eq(&p, &powerBuilder) => dead,
        Some(p) => p.try_borrow().is_ok_and(|s| s.done),
        None => lua_error!("bad argument #1 to 'status' (coroutine expected)"),
    };
    if dead {
        // L487
        ct.set(n!("powerBuilder"), Val::Nil);
        // L488
        if let Some(cb) = powerBuilderCallback(build) {
            // L489
            cb();
        }
    }
}

/// `coroutine.resume`: one step; an error kills the coroutine and is returned (`false, errMsg`).
fn resume(state: &mut PowerState, build: &BuildRef, options: &PowerOptions) -> Result<(), String> {
    if state.done {
        return Err("cannot resume dead coroutine".to_owned());
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        match pob_lua::error::catch(|| PowerBuilder(state, build, options)) {
            Ok(_) => Ok(()),
            Err(e) => {
                state.done = true;
                state.extra.set(n!("$iter"), Val::Nil);
                Err(e.0)
            }
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        // wasm is panic=abort (D15): an error ends the program as an uncaught Lua error would
        let _ = PowerBuilder(state, build, options);
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------
// PowerBuilder
// ---------------------------------------------------------------------------------------

/// Where the body resumes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Phase {
    /// L497-519 (before the first yield).
    Start,
    /// L518-604: the closures are defined; the node collection runs next.
    Collect,
    /// L606-663: `distanceList[di]`, its `ni`-th node.
    Distance {
        di: usize,
        ni: usize,
    },
    /// L665-710: `masteryNodeList[mi]`, its effect `ei` (1-based, as `ipairs`).
    Mastery {
        mi: usize,
        ei: usize,
    },
    /// L714-736: the `ci`-th `clusterNodeMap` pair.
    Cluster {
        ci: usize,
    },
    Done,
}

/// How the body calls `calcFunc` (CCR-0101 §4).
enum CalcMode {
    /// The misc calculator (sequential, D12).
    Live,
    /// Dry run: record each call, answer a placeholder output.
    Record(Vec<Job>),
    /// The recorded calls' results, in call order (parallel mode).
    Pre {
        results: std::collections::VecDeque<(JobKey, Result<SlimOut, String>)>,
    },
}

/// The body's locals between resumes.
struct PowerIter {
    phase: Phase,
    /// `coroutine.running()` (false for [`PowerBuilder_run`]).
    inCoroutine: bool,
    /// The run records its calls instead of being the report (no `self.*` writes).
    dryRun: bool,
    // L498
    useFullDPS: Option<bool>,
    // L499
    calcFunc: Rc<MiscCalculator>,
    calcBase: Output,
    // L500 (string keys: `node.modKey` and `node.modKey.."_remove"`)
    cache: ahash::AHashMap<LStr, Output>,
    // L502
    distanceList: Vec<(f64, Vec<(i64, NodeRef)>)>,
    // L503
    masteryNodeList: Vec<NodeRef>,
    // L504
    newPowerMax: Table,
    // L559
    start: f64,
    /// The last step ended at an L659/704/732 time yield: the next resume runs L660/705/733
    /// `start = GetTime()` before the loop goes on (the frame gap is not counted).
    restartClock: bool,
    // L560
    nodeIndex: f64,
    // L561
    total: f64,
    /// L710 inner loop: `node.masteryEffects or { }` of the current mastery node.
    masteryEffects: Option<Table>,
    /// L714 `pairs(clusterNodeMap)` snapshot.
    clusterList: Vec<Val>,
    calcMode: CalcMode,
    /// Mastery effect nodes built during a recording run → (mastery node id, effect id).
    effectNodes: Vec<(NodeRef, f64, f64)>,
}

/// @pob Classes/CalcsTab.lua:496-740 h=04c2908a
/// `CalcsTab:PowerBuilder()` (a coroutine in Lua): one resumable step per call; `None` when done.
pub fn PowerBuilder(
    state: &mut PowerState,
    build: &BuildRef,
    options: &PowerOptions,
) -> Option<()> {
    if state.done {
        return None;
    }
    let slot = match state
        .extra
        .obj::<RefCell<Option<Box<PowerIter>>>>(n!("$iter"))
    {
        Some(s) => s,
        None => {
            let s: Rc<RefCell<Option<Box<PowerIter>>>> = Rc::new(RefCell::new(None));
            state.extra.set(n!("$iter"), Val::Obj(s.clone()));
            s
        }
    };
    // the iterator is out of its slot while the body runs (a Lua error drops it with the step)
    let taken = slot.borrow_mut().take();
    let mut it = taken.unwrap_or_else(|| Box::new(PowerIter::start(build, true, false)));
    let yielded = it.step(build, options);
    if yielded {
        *slot.borrow_mut() = Some(it);
        Some(())
    } else {
        state.done = true;
        state.extra.set(n!("$iter"), Val::Nil);
        None
    }
}

/// `calcsTab:PowerBuilder()` called directly (outside a coroutine: `coroutine.running()` is nil,
/// so the body never yields). Lua errors propagate.
pub fn PowerBuilder_run(build: &BuildRef, options: &PowerOptions) {
    let mut it = Box::new(PowerIter::start(build, false, false));
    let yielded = it.step(build, options);
    debug_assert!(!yielded, "PowerBuilder_run never yields");
}

/// `self.powerStat and self.powerStat.stat and not self.powerStat.ignoreForNodes`.
fn singleStatMode(powerStat: &Option<Table>) -> bool {
    powerStat
        .as_ref()
        .is_some_and(|p| p.truthy(n!("stat")) && !p.truthy(n!("ignoreForNodes")))
}

/// `not self.powerStat or not self.powerStat.ignoreForNodes`.
fn offDefMode(powerStat: &Option<Table>) -> bool {
    powerStat
        .as_ref()
        .map_or(true, |p| !p.truthy(n!("ignoreForNodes")))
}

/// `node.power` (a spec node always has one: PassiveSpec.lua:56).
fn node_power(node: &Node) -> Table {
    match node.extra.get(n!("power")) {
        Val::Table(t) => t,
        other => lua_error!(
            "bad argument #1 to 'pairs' (table expected, got {})",
            other.lua_type()
        ),
    }
}

/// `a < b` with Lua's error on a nil operand.
fn lt_num(a: f64, b: Option<f64>) -> bool {
    match b {
        Some(b) => a < b,
        None => lua_error!("attempt to compare number with nil"),
    }
}

/// `self.mainEnv.grantedPassives[id]`.
fn granted(build: &BuildRef, id: f64) -> bool {
    let Some(mainEnv) = build.calcsTab.mainEnv.get() else {
        lua_error!("attempt to index field 'mainEnv' (a nil value)")
    };
    if id.is_nan() {
        return false;
    }
    mainEnv
        .grantedPassives
        .get()
        .with(|m| m.get(id as i64))
        .unwrap_or(false)
}

/// A node set (`{ [node] = true, … }`) in insertion order (CCR-0082 §2: id-keyed).
fn node_set(nodes: impl IntoIterator<Item = NodeRef>) -> Shared<LjIntMap<NodeRef>> {
    let mut m = LjIntMap::new();
    for n in nodes {
        m.set(n.id as i64, Some(n));
    }
    Shared::new(m)
}

fn add_override(set: Shared<LjIntMap<NodeRef>>) -> Rc<Override> {
    Rc::new(Override {
        addNodes: Field::new(Some(set)),
        ..Override::default()
    })
}

fn remove_override(set: Shared<LjIntMap<NodeRef>>) -> Rc<Override> {
    Rc::new(Override {
        removeNodes: Field::new(Some(set)),
        ..Override::default()
    })
}

impl PowerIter {
    /// L497-519.
    fn start(build: &BuildRef, inCoroutine: bool, dryRun: bool) -> PowerIter {
        let ps = powerStat(build);
        // L498
        let useFullDPS = ps
            .as_ref()
            .map(|p| p.str(n!("stat")).as_deref() == Some("FullDPS"));
        // L499
        let (calcFunc, calcBase) = GetMiscCalculator(build);
        PowerIter {
            phase: Phase::Start,
            inCoroutine,
            dryRun,
            useFullDPS,
            calcFunc,
            calcBase,
            cache: ahash::AHashMap::new(),
            distanceList: Vec::new(),
            masteryNodeList: Vec::new(),
            newPowerMax: Table::new(),
            start: 0.0,
            restartClock: false,
            nodeIndex: 0.0,
            total: 0.0,
            masteryEffects: None,
            clusterList: Vec::new(),
            calcMode: CalcMode::Live,
            effectNodes: Vec::new(),
        }
    }

    /// `calcFunc(override, useFullDPS)`.
    fn calc(&mut self, build: &BuildRef, ov: Rc<Override>) -> Output {
        match &mut self.calcMode {
            CalcMode::Live => {
                #[cfg(feature = "probe")]
                if let Some(out) = calc_replay::replay(&ov, self.useFullDPS) {
                    return out;
                }
                self.calcFunc.call(ov, self.useFullDPS)
            }
            CalcMode::Record(jobs) => {
                jobs.push(Job::from_override(
                    build,
                    &ov,
                    self.useFullDPS,
                    &self.effectNodes,
                ));
                placeholder_output()
            }
            CalcMode::Pre { results } => {
                let key = JobKey::of(&ov, self.useFullDPS);
                match results.pop_front() {
                    Some((k, r)) if k == key => match r {
                        Ok(slim) => slim.to_output(),
                        Err(e) => lua_error!("{e}"),
                    },
                    // the call sequence changed since the recording (`powerStat` changed mid-run):
                    // the rest runs on the live calculator
                    _ => {
                        self.calcMode = CalcMode::Live;
                        self.calcFunc.call(ov, self.useFullDPS)
                    }
                }
            }
        }
    }

    /// L559-604 + the loop bodies: runs until the next yield (`true`) or the end (`false`).
    fn step(&mut self, build: &BuildRef, options: &PowerOptions) -> bool {
        if self.restartClock {
            // L660 (705, 733): `start = GetTime()` right after `coroutine.yield()` returns
            self.restartClock = false;
            self.start = GetTime();
        }
        loop {
            match self.phase {
                Phase::Start => {
                    // L504
                    self.newPowerMax = Table::new();
                    self.newPowerMax.set(n!("singleStat"), 0.0);
                    self.newPowerMax.set(n!("offence"), 0.0);
                    self.newPowerMax.set(n!("offencePerPoint"), 0.0);
                    self.newPowerMax.set(n!("defence"), 0.0);
                    self.newPowerMax.set(n!("defencePerPoint"), 0.0);
                    // L511
                    if !self.dryRun && powerMax(build).is_none() {
                        // L512
                        ct_extra(build).set(n!("powerMax"), self.newPowerMax.clone());
                    }
                    self.phase = Phase::Collect;
                    // L514
                    if self.inCoroutine {
                        // L515
                        return true;
                    }
                }
                Phase::Collect => {
                    if options.parallel && !self.dryRun && matches!(self.calcMode, CalcMode::Live) {
                        self.calcMode = parallel_precompute(build);
                    }
                    self.collect(build);
                    self.phase = Phase::Distance { di: 0, ni: 0 };
                }
                Phase::Distance { di, ni } => {
                    // L606
                    if di >= self.distanceList.len() {
                        self.phase = Phase::Mastery { mi: 0, ei: 1 };
                        continue;
                    }
                    // L607
                    let distance = self.distanceList[di].0;
                    if ni == 0 {
                        // L608
                        if let Some(depth) = nodePowerMaxDepth(build) {
                            if depth < distance {
                                // L609
                                self.phase = Phase::Mastery { mi: 0, ei: 1 };
                                continue;
                            }
                        }
                    }
                    if ni >= self.distanceList[di].1.len() {
                        self.phase = Phase::Distance { di: di + 1, ni: 0 };
                        continue;
                    }
                    let (nodeId, node) = self.distanceList[di].1[ni].clone();
                    self.phase = Phase::Distance { di, ni: ni + 1 };
                    self.distanceNode(build, distance, nodeId, &node);
                    // L655
                    if self.yieldCheck(build) {
                        return true;
                    }
                }
                Phase::Mastery { mi, ei } => {
                    // L665
                    if mi >= self.masteryNodeList.len() {
                        // L714 LUA-ORDER: pairs(clusterNodeMap) (string keys: PoB's order is random
                        // per process; each node's power is independent of the order)
                        let tree = build.spec.get().tree.get();
                        self.clusterList = tree.clusterNodeMap.pairs().map(|(_, v)| v).collect();
                        self.phase = Phase::Cluster { ci: 0 };
                        continue;
                    }
                    let node = self.masteryNodeList[mi].clone();
                    if ei == 1 {
                        // L666 `ipairs(node.masteryEffects or { })`
                        self.masteryEffects = node.masteryEffects.get();
                    }
                    let masteryEffect = self
                        .masteryEffects
                        .as_ref()
                        .map_or(Val::Nil, |t| t.rawget(Key::I(ei as i64)));
                    if masteryEffect.is_nil() {
                        self.phase = Phase::Mastery { mi: mi + 1, ei: 1 };
                        continue;
                    }
                    self.phase = Phase::Mastery { mi, ei: ei + 1 };
                    if self.masteryEffect(build, &node, &masteryEffect) && self.yieldCheck(build) {
                        return true;
                    }
                }
                Phase::Cluster { ci } => {
                    if ci >= self.clusterList.len() {
                        // L737
                        if !self.dryRun {
                            ct_extra(build).set(n!("powerMax"), self.newPowerMax.clone());
                            // L738
                            ct_extra(build).set(n!("powerBuilderInitialized"), true);
                        }
                        // L739 (commented out)
                        self.phase = Phase::Done;
                        continue;
                    }
                    self.phase = Phase::Cluster { ci: ci + 1 };
                    let Some(node) = self.clusterList[ci].obj::<Node>() else {
                        lua_error!("clusterNodeMap holds a non-node value")
                    };
                    if self.clusterNode(build, &node) && self.yieldCheck(build) {
                        return true;
                    }
                }
                Phase::Done => return false,
            }
        }
    }

    /// L655-661 (and 700-706, 728-734): `nodeIndex` was counted; yield after 100 ms.
    fn yieldCheck(&mut self, build: &BuildRef) -> bool {
        // L655
        if self.inCoroutine && GetTime() - self.start > 100.0 {
            // L656
            if !self.dryRun {
                if let Some(cb) = powerBuilderProgressCallback(build) {
                    // L657
                    cb(m_floor(self.nodeIndex / self.total * 100.0));
                }
            }
            // L659 coroutine.yield(); L660 `start = GetTime()` runs on the next resume (`step`)
            self.restartClock = true;
            return true;
        }
        false
    }

    /// L559-604: the node lists and the eligible-node count.
    fn collect(&mut self, build: &BuildRef) {
        let spec = build.spec.get();
        // L559
        self.start = GetTime();
        // L560
        self.nodeIndex = 0.0;
        // L561
        self.total = 0.0;
        let nodePowerMaxDepth = nodePowerMaxDepth(build);

        // L501 distanceMap: dist → node-id map (insertion order of `pairs(spec.nodes)`)
        let mut distanceMap: Vec<(f64, LjIntMap<NodeRef>)> = Vec::new();
        // L563 LUA-ORDER: pairs(spec.nodes) fills the per-distance node maps, whose `pairs` order
        // decides which node of a shared modKey fills the cache (PoB: per process, §3.4)
        for (nodeId, node) in spec.nodes.with(LjIntMap::pairs) {
            // L564
            node_power(&node).wipe();
            // L565
            if node.r#type.get() == n!("Mastery") {
                // L566
                node_power(&node).set(n!("masteryEffects"), Table::new());
            }
            // L568
            if !node.modKey.get().is_empty() && !granted(build, nodeId as f64) {
                // L569
                if node.r#type.get() == n!("Mastery") && node.allMasteryOptions.get() {
                    // L570
                    let skip = match nodePowerMaxDepth {
                        Some(d) => lt_num(d, node.pathDist.get()),
                        None => false,
                    };
                    if !skip {
                        // L571
                        self.masteryNodeList.push(node.clone());
                        // L572
                        if let Some(me) = node.masteryEffects.get() {
                            for (_, masteryEffect) in me.ipairs() {
                                // L573
                                if masteryEffectCanBeAssignedToNode(build, &node, &masteryEffect) {
                                    // L574
                                    self.total += 1.0;
                                }
                            }
                        }
                    }
                } else {
                    // L579
                    let mut dist = node.pathDist.get().unwrap_or(1000.0);
                    // L580
                    if let Some(leaps) = node.extra.table(n!("intuitiveLeapLikesAffecting")) {
                        for (_, leap) in leaps.ipairs() {
                            let Some(leap) = leap.obj::<Node>() else {
                                lua_error!("attempt to index local 'leap' (a non-node value)")
                            };
                            // L581
                            if leap.alloc.get() {
                                // L582
                                dist =
                                    m_max(m_min(leap.pathDist.get().unwrap_or(1000.0), dist), 1.0);
                            }
                        }
                    }
                    // L585-586
                    let i = match distanceMap.iter().position(|(d, _)| *d == dist) {
                        Some(i) => i,
                        None => {
                            distanceMap.push((dist, LjIntMap::new()));
                            distanceMap.len() - 1
                        }
                    };
                    distanceMap[i].1.set(nodeId, Some(node.clone()));
                    // L587
                    node_power(&node).set(n!("distance"), dist);
                    // L588
                    if nodePowerMaxDepth.map_or(true, |d| dist <= d) {
                        // L589
                        self.total += 1.0;
                    }
                }
            }
        }
        // L594 (pairs(distanceMap) then the sort by distance: unique keys, so the order is total)
        self.distanceList = distanceMap
            .into_iter()
            .map(|(d, nodes)| (d, nodes.pairs()))
            .collect();
        // L598
        self.distanceList
            .sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        // Count eligible cluster nodes
        // L600
        let tree = spec.tree.get();
        for (_, node) in tree.clusterNodeMap.pairs() {
            let Some(node) = node.obj::<Node>() else {
                lua_error!("clusterNodeMap holds a non-node value")
            };
            // L601
            if !node.alloc.get() && !node.modKey.get().is_empty() && !granted(build, node.id) {
                // L602
                self.total += 1.0;
            }
        }
    }

    /// L537-557 `calculateAddNodePower(power, distance, node, output, buildPathNodes)`.
    fn calculateAddNodePower(
        &mut self,
        build: &BuildRef,
        power: &Table,
        distance: Option<f64>,
        node: &NodeRef,
        output: &Output,
        buildPathNodes: &dyn Fn() -> Shared<LjIntMap<NodeRef>>,
    ) {
        let ps = powerStat(build);
        // L538
        if singleStatMode(&ps) {
            let ps = ps.expect("powerStat");
            // L539
            power.set(
                n!("singleStat"),
                CalculatePowerStat(&ps, output, &self.calcBase),
            );
            // L540
            if node.path.get().is_some() && node.ascendancyName.get().is_none() {
                // L541
                let singleStat = power.num(n!("singleStat"));
                self.newPowerMax.set(
                    n!("singleStat"),
                    m_max(self.newPowerMax.num(n!("singleStat")), singleStat),
                );
                // L542
                power.set(n!("pathPower"), power.get(n!("singleStat")));
                // L543
                let Some(distance) = distance else {
                    lua_error!("attempt to compare nil with number")
                };
                if distance > 1.0 {
                    // L544
                    let out = self.calc(build, add_override(buildPathNodes()));
                    power.set(
                        n!("pathPower"),
                        CalculatePowerStat(&ps, &out, &self.calcBase),
                    );
                }
            }
        } else if offDefMode(&ps) {
            // L547 (elseif)
            // L548
            let (offence, defence) = CalculateCombinedOffDefStat(output, &self.calcBase);
            power.set(n!("offence"), offence);
            power.set(n!("defence"), defence);
            // L549
            power.set(n!("singleStat"), offence);
            // L550
            if node.path.get().is_some() && node.ascendancyName.get().is_none() {
                let m = &self.newPowerMax;
                // L551
                m.set(n!("offence"), m_max(m.num(n!("offence")), offence));
                // L552
                m.set(n!("defence"), m_max(m.num(n!("defence")), defence));
                let Some(distance) = distance else {
                    lua_error!("attempt to perform arithmetic on local 'distance' (a nil value)")
                };
                // L553
                m.set(
                    n!("offencePerPoint"),
                    m_max(m.num(n!("offencePerPoint")), offence / distance),
                );
                // L554
                m.set(
                    n!("defencePerPoint"),
                    m_max(m.num(n!("defencePerPoint")), defence / distance),
                );
            }
        }
    }

    /// L611-661: one node of the distance loop.
    fn distanceNode(&mut self, build: &BuildRef, distance: f64, nodeId: i64, node: &NodeRef) {
        let spec = build.spec.get();
        let modKey = node.modKey.get();
        // L612
        if !node.alloc.get() && !modKey.is_empty() && !granted(build, nodeId as f64) {
            // L613
            if !self.cache.contains_key(&modKey) {
                // L614
                let out = self.calc(build, add_override(node_set([node.clone()])));
                self.cache.insert(modKey.clone(), out);
            }
            // L616
            let output = self.cache[&modKey].clone();
            // L617
            let pathNode = node.clone();
            self.calculateAddNodePower(
                build,
                &node_power(node),
                Some(distance),
                node,
                &output,
                // L617-623
                &|| {
                    // L618
                    // L619
                    let path = pathNode
                        .path
                        .get()
                        .map(|p| p.snapshot())
                        .unwrap_or_default();
                    // L622
                    node_set(path)
                },
            );
        } else if node.alloc.get() && !modKey.is_empty() && !granted(build, nodeId as f64) {
            // L624
            let key = LStr::from(format!("{modKey}_remove"));
            if !self.cache.contains_key(&key) {
                // L626
                let out = self.calc(build, remove_override(node_set([node.clone()])));
                self.cache.insert(key.clone(), out);
            }
            // L628
            let output = self.cache[&key].clone();
            let ps = powerStat(build);
            // L629
            if singleStatMode(&ps) {
                let ps = ps.expect("powerStat");
                let power = node_power(node);
                // L630
                power.set(
                    n!("singleStat"),
                    CalculatePowerStat(&ps, &output, &self.calcBase),
                );
                // L631 (`node.depends` is always a table on a spec node)
                if node.ascendancyName.get().is_none() {
                    // L632
                    power.set(n!("pathPower"), power.get(n!("singleStat")));
                    // L633-636
                    let depends = node.depends.get().snapshot();
                    let pathNodes = node_set(depends.iter().cloned());
                    // L637
                    if depends.len() > 1 {
                        // L638
                        let out = self.calc(build, remove_override(pathNodes));
                        power.set(
                            n!("pathPower"),
                            CalculatePowerStat(&ps, &out, &self.calcBase),
                        );
                    }
                }
            }
        }
        // L643
        if node.r#type.get() == n!("Mastery") {
            // L644
            let selectedEffectId = spec.masterySelections.with(|m| m.get(node.id as i64));
            // L645
            if let Some(selectedEffectId) = selectedEffectId {
                let power = node_power(node);
                // L646-651
                let e = Table::new();
                e.set(n!("singleStat"), power.get(n!("singleStat")));
                e.set(n!("pathPower"), power.get(n!("pathPower")));
                e.set(n!("offence"), power.get(n!("offence")));
                e.set(n!("defence"), power.get(n!("defence")));
                let Some(me) = power.table(n!("masteryEffects")) else {
                    lua_error!("attempt to index field 'masteryEffects' (a nil value)")
                };
                me.set(Key::from(selectedEffectId), e);
            }
        }
        // L654
        self.nodeIndex += 1.0;
    }

    /// L666-708: one effect of a mastery node; `true` when it counted a node (`if effect`).
    fn masteryEffect(&mut self, build: &BuildRef, node: &NodeRef, masteryEffect: &Val) -> bool {
        // L667
        if !masteryEffectCanBeAssignedToNode(build, node, masteryEffect) {
            return false;
        }
        let tree = build.spec.get().tree.get();
        // L668
        let effectId = masteryEffect.get(n!("effect"));
        let effect = match &effectId {
            // `masteryEffects[nil]` is a read: nil, so L669 `if effect` skips it
            Val::Nil => return false,
            v => tree.masteryEffects.get(Key::from(v.num())),
        };
        // L669
        let Some(effect) = effect.obj::<Node>() else {
            return false;
        };
        // L670
        let effectNode = buildMasteryEffectNode(build, node, &effect);
        if matches!(self.calcMode, CalcMode::Record(_)) {
            self.effectNodes
                .push((effectNode.clone(), node.id, effect.id));
        }
        // L671
        if !effectNode.modKey.get().is_empty() {
            let modKey = effectNode.modKey.get();
            // L672
            if !self.cache.contains_key(&modKey) {
                // L673
                let out = self.calc(build, add_override(node_set([effectNode.clone()])));
                self.cache.insert(modKey.clone(), out);
            }
            // L675
            let output = self.cache[&modKey].clone();
            let power = node_power(node);
            let Some(me) = power.table(n!("masteryEffects")) else {
                lua_error!("attempt to index field 'masteryEffects' (a nil value)")
            };
            // L676
            me.set(Key::from(effect.id), Table::new());
            // L677
            let effectPower = me.table(Key::from(effect.id)).expect("effectPower");
            // L678
            let pathTarget = node.clone();
            let pathEffect = effectNode.clone();
            self.calculateAddNodePower(
                build,
                &effectPower,
                node.pathDist.get(),
                node,
                &output,
                // L678-688
                &|| {
                    // L679-681
                    let mut list = vec![pathEffect.clone()];
                    // L682
                    for pathNode in pathTarget
                        .path
                        .get()
                        .map(|p| p.snapshot())
                        .unwrap_or_default()
                    {
                        // L683
                        if !Rc::ptr_eq(&pathNode, &pathTarget) {
                            // L684
                            list.push(pathNode);
                        }
                    }
                    // L687
                    node_set(list)
                },
            );
            let ps = powerStat(build);
            // L689
            if singleStatMode(&ps) {
                // L690
                if !effectPower.truthy(n!("pathPower")) {
                    effectPower.set(n!("pathPower"), effectPower.get(n!("singleStat")));
                }
                // L691
                power.set(
                    n!("singleStat"),
                    m_max(
                        power.get(n!("singleStat")).opt_num().unwrap_or(0.0),
                        effectPower.num(n!("singleStat")),
                    ),
                );
                // L692
                power.set(
                    n!("pathPower"),
                    m_max(
                        power.get(n!("pathPower")).opt_num().unwrap_or(0.0),
                        effectPower.num(n!("pathPower")),
                    ),
                );
            } else if offDefMode(&ps) {
                // L693 (elseif)
                // L694
                power.set(
                    n!("offence"),
                    m_max(
                        power.get(n!("offence")).opt_num().unwrap_or(0.0),
                        effectPower.num(n!("offence")),
                    ),
                );
                // L695
                power.set(
                    n!("defence"),
                    m_max(
                        power.get(n!("defence")).opt_num().unwrap_or(0.0),
                        effectPower.num(n!("defence")),
                    ),
                );
                // L696
                power.set(
                    n!("singleStat"),
                    m_max(
                        power.get(n!("singleStat")).opt_num().unwrap_or(0.0),
                        effectPower.num(n!("singleStat")),
                    ),
                );
            }
        }
        // L699
        self.nodeIndex += 1.0;
        true
    }

    /// L715-735: one cluster notable; `true` when it counted a node.
    fn clusterNode(&mut self, build: &BuildRef, node: &NodeRef) -> bool {
        // L715
        if !node.extra.get(n!("power")).truthy() {
            // L716
            node.extra.set(n!("power"), Table::new());
        }
        // L718
        node_power(node).wipe();
        let modKey = node.modKey.get();
        // L719
        if !node.alloc.get() && !modKey.is_empty() && !granted(build, node.id) {
            // L720
            if !self.cache.contains_key(&modKey) {
                // L721
                let out = self.calc(build, add_override(node_set([node.clone()])));
                self.cache.insert(modKey.clone(), out);
            }
            // L723
            let output = self.cache[&modKey].clone();
            let ps = powerStat(build);
            // L724
            if singleStatMode(&ps) {
                let ps = ps.expect("powerStat");
                // L725
                node_power(node).set(
                    n!("singleStat"),
                    CalculatePowerStat(&ps, &output, &self.calcBase),
                );
            }
            // L727
            self.nodeIndex += 1.0;
            return true;
        }
        false
    }
}

/// L518-530 `buildMasteryEffectNode(node, effect)`.
fn buildMasteryEffectNode(build: &BuildRef, node: &Node, effect: &Node) -> NodeRef {
    // L519-524
    let effectNode = new_node_table(node.id);
    effectNode.r#type.set(node.r#type.get());
    effectNode.extra.set(n!("name"), node.extra.get(n!("name")));
    // L525
    let sd: Vec<LStr> = effect.sd.get().snapshot();
    // L526
    effectNode.sd.set(Shared::new(sd));
    let effectNode = Rc::new(effectNode);
    // L528
    build.spec.get().tree.get().ProcessStats(&effectNode, None);
    // L529
    effectNode
}

/// L532-535 `masteryEffectCanBeAssignedToNode(node, masteryEffect)`.
fn masteryEffectCanBeAssignedToNode(build: &BuildRef, node: &Node, masteryEffect: &Val) -> bool {
    let effect = masteryEffect.get(n!("effect"));
    // L533 isValueInTable(self.build.spec.masterySelections, masteryEffect.effect)
    // (LUA-ORDER: the first key in pairs order; a selected effect is held by one mastery)
    let assignedNodeId = build.spec.get().masterySelections.with(|m| {
        m.pairs()
            .into_iter()
            .find(|(_, v)| effect.opt_num() == Some(*v))
            .map(|(k, _)| k)
    });
    // L534
    match assignedNodeId {
        None => true,
        Some(id) => id as f64 == node.id,
    }
}

// ---------------------------------------------------------------------------------------
// The power stats
// ---------------------------------------------------------------------------------------

/// @pob Classes/CalcsTab.lua:742-746 h=8e17c3b0
pub fn CalculatePowerStat(selection: &Table, original: &Output, modified: &Output) -> f64 {
    // L743
    let originalValue = GetFromOutput(original, selection, false);
    // L744
    let modifiedValue = GetFromOutput(modified, selection, false);
    // L745
    originalValue - modifiedValue
}

/// `output.Minion and output.Minion[k] or 0`.
fn minion_or0(output: &Output, k: &str) -> Val {
    let minion = output.get(n!("Minion"));
    let v = land(minion.clone(), || minion.get(Name::from(k)));
    lor(v, || Val::Num(0.0))
}

/// `output[k] or 0`.
fn or0(output: &Output, k: &str) -> Val {
    lor(output.get(Name::from(k)), || Val::Num(0.0))
}

/// @pob Modules/Data.lua:186-212 h=0cefea12
/// `data.powerStatList.GetFromOutput(output, statTable, skipTransform)` (called by
/// `CalculatePowerStat`; `statTable` is a `data.powerStatList` entry or a `{ stat = … }` table).
/// The Lua value is returned as an arithmetic operand (a non-number raises here instead of at
/// the caller's arithmetic).
pub fn GetFromOutput(output: &Output, statTable: &Table, skipTransform: bool) -> f64 {
    // L187-208 getEntry()
    let getEntry = || -> Val {
        let stat = statTable.get(n!("stat"));
        // L188
        if stat.str().as_deref() == Some("FullDPS") {
            // L189
            if !output.get(n!("FullDPS")).is_nil() {
                // L190
                return or0(output, "FullDPS");
            }
            // if the user doesn't have full dps, we default to adding the player and minion dps together
            // L193
            return Val::Num(
                or0(output, "CombinedDPS").num() + minion_or0(output, "CombinedDPS").num(),
            );
        }
        let Some(stat) = stat.str() else {
            lua_error!(
                "attempt to index field 'stat' (a {} value)",
                stat.lua_type()
            )
        };
        // minion-only stats
        // L196
        let minionStat = pob_lua::pattern::r#match(&stat, "^Minion(.+)", None).and_then(|caps| {
            match caps.into_iter().next() {
                Some(pob_lua::pattern::Capture::Str(b)) => {
                    Some(String::from_utf8_lossy(b).into_owned())
                }
                _ => None,
            }
        });
        // L197
        if let Some(minionStat) = minionStat {
            // L198
            return minion_or0(output, &minionStat);
        }
        // damage stats use a combination of player and minion dps
        // L201-202
        let isDamageStat = &*stat == "AverageDamage"
            || &*stat == "TotalDot"
            || pob_lua::pattern::find(&stat, "DPS", None, false).is_some();
        // L203
        if isDamageStat {
            // L204
            return Val::Num(or0(output, &stat).num() + minion_or0(output, &stat).num());
        }
        // L206
        or0(output, &stat)
    };
    // L209
    let transform = statTable.get(n!("transform"));
    if transform.truthy() && !skipTransform {
        let Some(f) = transform.obj::<LuaFnRef>() else {
            lua_error!(
                "attempt to call field 'transform' (a {} value)",
                transform.lua_type()
            )
        };
        // L210
        return powerStatTransform(&f.id, getEntry()).num();
    }
    // L212
    getEntry().num()
}

/// @pob Modules/Data.lua:131-131 h=107327e2
/// @pob Modules/Data.lua:164-168 h=e900d657
/// The `transform` closures of `data.powerStatList` entries (`PowerStat::transform`), by
/// closure id: `Name` strips a leading "The "; the five `*TakenHit` stats negate.
pub fn powerStatTransform(fnId: &str, value: Val) -> Val {
    match fnId {
        // L131 function(value) return value:gsub("^The ","") end (first result; only strings
        // have a metatable in LuaJIT, so `value:gsub` on a number raises like any other type)
        "data.powerStatList[2].transform" => {
            let s = match &value {
                Val::Str(s) => s.to_string(),
                other => lua_error!(
                    "attempt to index local 'value' (a {} value)",
                    other.lua_type()
                ),
            };
            Val::from(gsub(&s, "^The ", "", None).0)
        }
        // L164-168 function(value) return -value end
        "data.powerStatList[35].transform"
        | "data.powerStatList[36].transform"
        | "data.powerStatList[37].transform"
        | "data.powerStatList[38].transform"
        | "data.powerStatList[39].transform" => Val::Num(-value.num()),
        other => lua_error!("powerStatTransform: unknown closure {other}"),
    }
}

/// @pob Classes/CalcsTab.lua:748-758 h=d5dfc379
/// → `(offence, defence)`: both Lua results (CCR-0101 §2).
pub fn CalculateCombinedOffDefStat(original: &Output, modified: &Output) -> (f64, f64) {
    // `x.EnergyShieldRecoveryCap or x.EnergyShield`
    let esCap = |o: &Output| {
        lor(o.get(n!("EnergyShieldRecoveryCap")), || {
            o.get(n!("EnergyShield"))
        })
        .num()
    };
    // L749-754
    let defence = (original.num(n!("LifeUnreserved")) - modified.num(n!("LifeUnreserved")))
        / m_max(3000.0, modified.num(n!("Life")))
        + (original.num(n!("Armour")) - modified.num(n!("Armour")))
            / m_max(10000.0, modified.num(n!("Armour")))
        + (esCap(original) - esCap(modified)) / m_max(3000.0, esCap(modified))
        + (original.num(n!("Evasion")) - modified.num(n!("Evasion")))
            / m_max(10000.0, modified.num(n!("Evasion")))
        + (original.num(n!("LifeRegenRecovery")) - modified.num(n!("LifeRegenRecovery"))) / 500.0
        + (original.num(n!("EnergyShieldRegenRecovery"))
            - modified.num(n!("EnergyShieldRegenRecovery")))
            / 1000.0;
    // `x.Minion and x.Minion.CombinedDPS or 0`
    let minionDps = |o: &Output| {
        let m = o.get(n!("Minion"));
        lor(land(m.clone(), || m.get(n!("CombinedDPS"))), || {
            Val::Num(0.0)
        })
        .num()
    };
    // L755
    let modifiedDps = modified.num(n!("CombinedDPS")) + minionDps(modified);
    // L756
    let dpsIncr = original.num(n!("CombinedDPS")) + minionDps(original) - modifiedDps;
    // L757
    (dpsIncr / modifiedDps, defence)
}

/// @pob Classes/CalcsTab.lua:760-762 h=49183536
/// `self.miscCalculator[1], self.miscCalculator[2]` (set by [`BuildOutput`]).
pub fn GetMiscCalculator(build: &BuildRef) -> (Rc<MiscCalculator>, Output) {
    // L761
    let Some(f) = build.calcsTab.miscCalculator.get() else {
        lua_error!("attempt to index field 'miscCalculator' (a nil value)")
    };
    let base = ct_extra(build)
        .table(n!("miscCalculatorBase"))
        .unwrap_or_else(|| lua_error!("attempt to index field 'miscCalculator' (a nil value)"));
    (f, base)
}

/// @pob Classes/CalcsTab.lua:764-766 h=ced0e4c0
/// UI undo state (no calc effect).
pub fn CreateUndoState(build: &BuildRef) -> Table {
    // L765
    build.calcsTab.input.copyTable(false)
}

/// @pob Classes/CalcsTab.lua:768-773 h=f936be82
/// UI undo state (no calc effect).
pub fn RestoreUndoState(build: &BuildRef, state: &Table) {
    let input = &build.calcsTab.input;
    // L769
    input.wipe();
    // L770
    for (k, v) in state.pairs() {
        // L771
        input.set(k, v);
    }
}

// ---------------------------------------------------------------------------------------
// Parallel mode (CCR-0101 §4; D12): Rust-only plumbing
// ---------------------------------------------------------------------------------------

/// A node of a recorded calc call, as a worker finds it in its own build.
#[derive(Clone, Debug, PartialEq)]
enum NodeSpec {
    /// `spec.nodes[id]`.
    Spec(f64),
    /// `tree.clusterNodeMap[name]`.
    Cluster(String),
    /// `buildMasteryEffectNode(spec.nodes[node], tree.masteryEffects[effect])`.
    Effect { node: f64, effect: f64 },
}

/// The identity of a calc call: add/remove, the node set (id, modKey) in order, `useFullDPS`.
#[derive(Clone, Debug, PartialEq)]
struct JobKey {
    remove: bool,
    nodes: Vec<(i64, String)>,
    useFullDPS: Option<bool>,
}

impl JobKey {
    fn of(ov: &Override, useFullDPS: Option<bool>) -> JobKey {
        let (remove, set) = match (ov.addNodes.get(), ov.removeNodes.get()) {
            (Some(s), _) => (false, s),
            (None, Some(s)) => (true, s),
            (None, None) => (false, Shared::new(LjIntMap::new())),
        };
        let nodes = set.with(LjIntMap::pairs);
        JobKey {
            remove,
            nodes: nodes
                .into_iter()
                .map(|(k, n)| (k, n.modKey.get().to_string()))
                .collect(),
            useFullDPS,
        }
    }
}

/// One recorded calc call.
#[derive(Clone, Debug)]
struct Job {
    key: JobKey,
    nodes: Vec<NodeSpec>,
}

impl Job {
    fn from_override(
        build: &BuildRef,
        ov: &Override,
        useFullDPS: Option<bool>,
        effectNodes: &[(NodeRef, f64, f64)],
    ) -> Job {
        let key = JobKey::of(ov, useFullDPS);
        let set = ov
            .addNodes
            .get()
            .or_else(|| ov.removeNodes.get())
            .unwrap_or_else(|| Shared::new(LjIntMap::new()));
        let spec = build.spec.get();
        let tree = spec.tree.get();
        let nodes = set
            .with(LjIntMap::pairs)
            .into_iter()
            .map(|(id, n)| {
                if spec
                    .nodes
                    .with(|m| m.get(id))
                    .is_some_and(|s| Rc::ptr_eq(&s, &n))
                {
                    NodeSpec::Spec(n.id)
                } else if let Some((_, node, effect)) =
                    effectNodes.iter().find(|(e, _, _)| Rc::ptr_eq(e, &n))
                {
                    NodeSpec::Effect {
                        node: *node,
                        effect: *effect,
                    }
                } else {
                    let name = n.dn.get().to_string();
                    let found = tree
                        .clusterNodeMap
                        .get(Name::from(name.as_str()))
                        .obj::<Node>()
                        .is_some_and(|c| Rc::ptr_eq(&c, &n));
                    if !found {
                        lua_error!(
                            "PowerBuilder (parallel): node {} has no worker identity",
                            n.id
                        )
                    }
                    NodeSpec::Cluster(name)
                }
            })
            .collect();
        Job { key, nodes }
    }
}

/// A scalar output value a worker sends back.
#[derive(Clone, Debug)]
enum SlimVal {
    Num(f64),
    Bool(bool),
    Str(String),
}

/// The output keys the body reads (CalculateCombinedOffDefStat, GetFromOutput), from a worker.
#[derive(Clone, Debug, Default)]
struct SlimOut {
    top: Vec<(String, SlimVal)>,
    minion: Option<Vec<(String, SlimVal)>>,
}

const SLIM_KEYS: &[&str] = &[
    "LifeUnreserved",
    "Life",
    "Armour",
    "EnergyShieldRecoveryCap",
    "EnergyShield",
    "Evasion",
    "LifeRegenRecovery",
    "EnergyShieldRegenRecovery",
    "CombinedDPS",
    "FullDPS",
    "FullDotDPS",
];

fn slim_val(v: &Val) -> Option<SlimVal> {
    match v {
        Val::Num(x) => Some(SlimVal::Num(*x)),
        Val::Bool(b) => Some(SlimVal::Bool(*b)),
        Val::Str(s) => Some(SlimVal::Str(s.to_string())),
        _ => None,
    }
}

impl SlimOut {
    fn of(output: &Output, keys: &[String]) -> SlimOut {
        let pick = |t: &Table| {
            keys.iter()
                .filter_map(|k| slim_val(&t.get(Name::from(k.as_str()))).map(|v| (k.clone(), v)))
                .collect::<Vec<_>>()
        };
        SlimOut {
            top: pick(output),
            minion: output.table(n!("Minion")).map(|m| pick(&m)),
        }
    }

    fn to_output(&self) -> Output {
        let fill = |t: &Table, kv: &[(String, SlimVal)]| {
            for (k, v) in kv {
                let v = match v {
                    SlimVal::Num(x) => Val::Num(*x),
                    SlimVal::Bool(b) => Val::Bool(*b),
                    SlimVal::Str(s) => Val::from(&**s),
                };
                t.set(Name::from(k.as_str()), v);
            }
        };
        let out = Table::new();
        fill(&out, &self.top);
        if let Some(m) = &self.minion {
            let mt = Table::new();
            fill(&mt, m);
            out.set(n!("Minion"), mt);
        }
        out
    }
}

/// The dry run's answer to every call: the keys the body reads, all zero.
fn placeholder_output() -> Output {
    let out = Table::new();
    for k in SLIM_KEYS {
        out.set(Name::from(k), 0.0);
    }
    out
}

/// The keys a worker returns for the current `powerStat`.
fn slim_keys(build: &BuildRef) -> Vec<String> {
    let mut keys: Vec<String> = SLIM_KEYS.iter().map(|k| (*k).to_owned()).collect();
    if let Some(stat) = powerStat(build).and_then(|p| p.str(n!("stat"))) {
        keys.push(stat.to_string());
        if let Some(rest) = stat.strip_prefix("Minion").filter(|r| !r.is_empty()) {
            keys.push(rest.to_owned());
        }
    }
    keys
}

/// Worker threads (each holds a whole build and its trees).
const MAX_WORKERS: usize = 4;

/// Records the body's calc calls (a dry run), runs them on worker threads and returns them in
/// call order. Falls back to the live calculator when the build cannot be saved or no worker
/// thread is available.
fn parallel_precompute(build: &BuildRef) -> CalcMode {
    // the dry run: the same body, recording, with no `self.*` writes and no yields
    let mut dry = PowerIter::start(build, false, true);
    dry.calcMode = CalcMode::Record(Vec::new());
    while dry.step(build, &PowerOptions::default()) {}
    let CalcMode::Record(jobs) = std::mem::replace(&mut dry.calcMode, CalcMode::Live) else {
        unreachable!("recording run")
    };
    #[cfg(target_arch = "wasm32")]
    {
        let _ = jobs;
        return CalcMode::Live;
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let Some(xml) = build.SaveDB("") else {
            return CalcMode::Live;
        };
        let keys = slim_keys(build);
        let workers = std::thread::available_parallelism()
            .map_or(1, std::num::NonZeroUsize::get)
            .clamp(1, MAX_WORKERS)
            .min(jobs.len().div_ceil(8).max(1));
        let specs: Vec<(usize, Vec<NodeSpec>, bool, Option<bool>)> = jobs
            .iter()
            .enumerate()
            .map(|(i, j)| (i, j.nodes.clone(), j.key.remove, j.key.useFullDPS))
            .collect();
        let mut results: Vec<Option<Result<SlimOut, String>>> = vec![None; jobs.len()];
        std::thread::scope(|s| {
            let handles: Vec<_> = (0..workers)
                .map(|w| {
                    let mine: Vec<_> = specs
                        .iter()
                        .filter(|(i, ..)| i % workers == w)
                        .cloned()
                        .collect();
                    let xml = xml.clone();
                    let keys = keys.clone();
                    s.spawn(move || worker(&xml, &mine, &keys))
                })
                .collect();
            for h in handles {
                match h.join() {
                    Ok(Ok(rs)) => {
                        for (i, r) in rs {
                            results[i] = Some(r);
                        }
                    }
                    Ok(Err(e)) => {
                        for r in results.iter_mut().filter(|r| r.is_none()) {
                            *r = Some(Err(e.clone()));
                        }
                    }
                    Err(_) => {}
                }
            }
        });
        CalcMode::Pre {
            results: jobs
                .into_iter()
                .zip(results)
                .map(|(j, r)| {
                    (
                        j.key,
                        r.unwrap_or_else(|| {
                            Err("PowerBuilder (parallel): worker failed".to_owned())
                        }),
                    )
                })
                .collect(),
        }
    }
}

/// One worker: its own build from the XML (`Build:Init`, including `BuildOutput`), then each
/// job's `calcFunc` call. A job's Lua error is its result; a load error fails every job.
#[cfg(not(target_arch = "wasm32"))]
fn worker(
    xml: &str,
    jobs: &[(usize, Vec<NodeSpec>, bool, Option<bool>)],
    keys: &[String],
) -> Result<Vec<(usize, Result<SlimOut, String>)>, String> {
    let build = pob_lua::error::catch(|| crate::model::build_xml::loadBuildFromXML(xml, data()))
        .map_err(|e| format!("PowerBuilder (parallel): worker build: {}", e.0))?;
    let (calcFunc, _) = pob_lua::error::catch(|| GetMiscCalculator(&build))
        .map_err(|e| format!("PowerBuilder (parallel): worker calculator: {}", e.0))?;
    let mut out = Vec::with_capacity(jobs.len());
    for (i, nodes, remove, useFullDPS) in jobs {
        let r = pob_lua::error::catch(|| {
            let spec = build.spec.get();
            let tree = spec.tree.get();
            let list: Vec<NodeRef> = nodes
                .iter()
                .map(|ns| match ns {
                    NodeSpec::Spec(id) => spec
                        .nodes
                        .with(|m| m.get(*id as i64))
                        .unwrap_or_else(|| lua_error!("worker: no spec node {id}")),
                    NodeSpec::Cluster(name) => tree
                        .clusterNodeMap
                        .get(Name::from(name.as_str()))
                        .obj::<Node>()
                        .unwrap_or_else(|| lua_error!("worker: no cluster node {name}")),
                    NodeSpec::Effect { node, effect } => {
                        let n = spec
                            .nodes
                            .with(|m| m.get(*node as i64))
                            .unwrap_or_else(|| lua_error!("worker: no spec node {node}"));
                        let e = tree
                            .masteryEffects
                            .get(Key::from(*effect))
                            .obj::<Node>()
                            .unwrap_or_else(|| lua_error!("worker: no mastery effect {effect}"));
                        buildMasteryEffectNode(&build, &n, &e)
                    }
                })
                .collect();
            let set = node_set(list);
            let ov = if *remove {
                remove_override(set)
            } else {
                add_override(set)
            };
            SlimOut::of(&calcFunc.call(ov, *useFullDPS), keys)
        })
        .map_err(|e| e.0);
        out.push((*i, r));
    }
    Ok(out)
}
