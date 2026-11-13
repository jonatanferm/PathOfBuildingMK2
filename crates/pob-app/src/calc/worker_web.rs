//! `WorkerCalc`: the wasm Web Worker client of the calc (ui doc §10.4, calc doc D15). Owner: U54
//! (optional). Not a parity target.
//!
//! lua: Classes/CalcsTab.lua:441-474 (BuildOutput), :760-762 (GetMiscCalculator),
//! Modules/Build.lua:1258-1272 and :691-697 (where PoB recalculates), Launch.lua:112-118 (the
//! `PCall` that turns a calc error into an error prompt). The calc itself is calc:PWR's
//! (`pob_calc::modules::power`); this file adapts it to the facade and to a Web Worker, so it
//! carries no `@pob` markers.
//!
//! # Why a worker (D15)
//!
//! wasm32-unknown-unknown is `panic=abort`: a Lua error (`lua_error!`, a panic with a `LuaError`
//! payload) ends the program, where PoB's `PCall` (Launch.lua:112) shows the error and keeps the
//! build open. The worker is the calc's own address space: it loads the build from its XML text
//! (calc:BM-A), runs `calcsTab:BuildOutput()` and answers. When the calc raises, only the worker
//! dies; the client records the error and starts a new worker.
//!
//! # The protocol (CCR-0134 §2)
//!
//! Messages are JSON strings (`postMessage` of a JS string).
//!
//! | Direction | Message | Meaning |
//! |---|---|---|
//! | client → worker | `{"op":"build_output","seq":n,"xml":"<PathOfBuilding>…"}` | `build:SaveDB()` of the live model; the worker runs `loadBuildModelFromXML`, `wipeGlobalCache()`, `skillsTab:UpdateSocketGroups()` and `calcsTab:BuildOutput()` (Build.lua:691-695, 1260-1266) |
//! | worker → client | `{"ready":true}` | booted (game data loaded) |
//! | worker → client | `{"seq":n,"ok":true,"stats":{"Life":5021,…}}` | the calc finished; `stats` are the numeric top-level entries of `calcsTab.mainOutput` (non-finite values as `"nan"`, `"inf"`, `"-inf"`) |
//! | worker → client | `{"seq":n,"ok":false,"error":"…"}` | the calc raised (`LuaError` text); posted by the worker's panic hook just before the abort |
//! | worker → client | `{"dead":true,"error":"…"}` | the worker trapped (after the error reply) and closed itself: its script posts it from `run`'s catch, or from its global `error`/`unhandledrejection` listeners when the trap is thrown from a wasm-bindgen-futures task; the page's `Worker.onerror` ([`WorkerEvent::Error`]) is the fallback route |
//! | worker → client | `{"bootError":"…"}` | the worker could not load the game data and closed itself |
//!
//! One request is in flight at a time. A newer XML waits in a one-slot queue (newest wins).
//!
//! # What the client does with an answer
//!
//! The UI reads the calc's object graph directly (`calcsTab.mainEnv.player.output`,
//! `.breakdown`, `.modDB`, `mainSkill.skillFlags`, `node.finalModList`, …; ui doc §10.4), and that
//! graph shares objects with the live model (socket groups, gem instances, items, nodes). It does
//! not cross a `postMessage`: the one encoder/injector pair of these types (calc:ORC-X
//! `probe::export_root` and `pob_oracle::inject`) is desktop-only (pob-oracle links LuaJIT) and
//! has no re-pointing step for the live model outside `LuaCalcBridge` (CCR-0134 §3). So the
//! worker is the calc's **pre-flight**: for a model whose XML the worker calculated without an
//! error, the client runs the same `BuildOutput` on the live model and returns `Ready`; that run
//! is the one the UI reads. It cross-checks the worker's `stats` against the live
//! `mainOutput` ([`WorkerStatus::mismatches`]).
//!
//! * Before the worker has answered for the current XML, `build_output` returns `Pending`: the
//!   frame keeps the previous results (Build.lua:1266-1272 are skipped, on_frame.rs). When the
//!   answer arrives the client raises `build.buildFlag` again, so the next frame recalculates and
//!   gets `Ready`.
//! * When the worker's calc raised for the current XML, `build_output` stays `Pending` for that
//!   XML: the build keeps its last good results, as in PoB, where the error aborts the frame
//!   before `mainEnv` is replaced. The next change of the model asks again.
//! * When a worker dies without answering the request in flight, that death is the request's
//!   verdict (as a calc error) only when the worker had booted, had not answered with an error
//!   before (a panic-hook reply is followed by the death, so a request posted after it was
//!   posted to a dying worker), and the XML was already asked again once. Otherwise the XML is
//!   asked of the next worker (unless a newer one waits).
//! * A model change takes two frames on the web: the `Pending` frame and the `Ready` frame. Both
//!   run Build.lua:1260-1265 (`wipeGlobalCache`, `UpdateSocketGroups`, `outputRevision += 1`),
//!   so `outputRevision` rises by 2 per change (not a parity target). Its one reader,
//!   Launch.lua:114 (`== 1`: a crash on the initial load), sees the first calc, which runs on the
//!   main thread in one frame.
//! * The first calc of a build (no `calcsTab.mainEnv` yet: Build:Init, Build.lua:695) runs on the
//!   main thread directly (CCR-0134 §4: Init and the stat list read `mainEnv` right after it and
//!   have no placeholder for `Pending`).
//! * When the worker cannot boot twice in a row, the client falls back to the main-thread calc
//!   (`WorkerStatus::broken`), as `RustCalc` does.
//!
//! `misc_calculator` and `MiscCalc::call` run on the main thread on the calculator the last
//! `Ready` installed (`calcsTab.miscCalculator`, CalcsTab.lua:470-471); before the first one they
//! return `Pending`.
//!
//! DIVERGENCE(worker-error): PoB shows a calc error as Launch's prompt ("In 'OnFrame': …",
//! Launch.lua:118). The facade has no `Cx`, so the client logs it to the console and keeps it in
//! [`WorkerStatus::errors`] (CCR-0134 §5 proposes a facade method for the prompt).
//! DIVERGENCE(worker-misc): `MiscCalc::call` (tooltips, the power report) is not pre-flighted: an
//! override's calc that raises still aborts the page.
//!
//! Desktop: [`ThreadHub`] runs the same request handler on threads (one thread per worker, a Lua
//! error ends the thread after its error reply), for tests and as a dev option.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::{Rc, Weak};
use std::task::Poll;

use pob_calc::modules::power;
use serde_json::{json, Value as J};

use super::facade::{BuildRef, CalcFacade, MiscCalc, Output};
use super::rust::RustMiscCalc;
use crate::model;

// ---- protocol ---------------------------------------------------------------------------------

/// The protocol's messages (module docs, CCR-0134 §2).
pub mod protocol {
    use super::{json, BTreeMap, J};

    /// The numeric top-level entries of `calcsTab.mainOutput`.
    pub type Stats = BTreeMap<String, f64>;

    /// client → worker.
    #[derive(Clone, Debug, PartialEq)]
    pub enum Request {
        BuildOutput { seq: u64, xml: String },
    }

    impl Request {
        pub fn seq(&self) -> u64 {
            match self {
                Request::BuildOutput { seq, .. } => *seq,
            }
        }
    }

    /// worker → client.
    #[derive(Clone, Debug, PartialEq)]
    pub enum Reply {
        Ready,
        Done {
            seq: u64,
            result: Result<Stats, String>,
        },
        Dead {
            error: String,
        },
        BootError {
            error: String,
        },
    }

    pub fn encode_request(r: &Request) -> String {
        match r {
            Request::BuildOutput { seq, xml } => {
                json!({ "op": "build_output", "seq": seq, "xml": xml }).to_string()
            }
        }
    }

    pub fn decode_request(s: &str) -> Result<Request, String> {
        let v: J = serde_json::from_str(s).map_err(|e| format!("bad request: {e}"))?;
        let seq = v["seq"].as_u64().ok_or("bad request: no seq")?;
        match v["op"].as_str() {
            Some("build_output") => Ok(Request::BuildOutput {
                seq,
                xml: v["xml"].as_str().ok_or("bad request: no xml")?.to_owned(),
            }),
            op => Err(format!("bad request: op {op:?}")),
        }
    }

    fn num_json(x: f64) -> J {
        if x.is_nan() {
            J::from("nan")
        } else if x == f64::INFINITY {
            J::from("inf")
        } else if x == f64::NEG_INFINITY {
            J::from("-inf")
        } else {
            json!(x)
        }
    }

    fn json_num(v: &J) -> Option<f64> {
        match v {
            J::Number(n) => n.as_f64(),
            J::String(s) => match s.as_str() {
                "nan" => Some(f64::NAN),
                "inf" => Some(f64::INFINITY),
                "-inf" => Some(f64::NEG_INFINITY),
                _ => None,
            },
            _ => None,
        }
    }

    pub fn encode_reply(r: &Reply) -> String {
        match r {
            Reply::Ready => json!({ "ready": true }),
            Reply::Done {
                seq,
                result: Ok(stats),
            } => {
                let m: serde_json::Map<String, J> = stats
                    .iter()
                    .map(|(k, v)| (k.clone(), num_json(*v)))
                    .collect();
                json!({ "seq": seq, "ok": true, "stats": m })
            }
            Reply::Done {
                seq,
                result: Err(e),
            } => json!({ "seq": seq, "ok": false, "error": e }),
            Reply::Dead { error } => json!({ "dead": true, "error": error }),
            Reply::BootError { error } => json!({ "bootError": error }),
        }
        .to_string()
    }

    pub fn decode_reply(s: &str) -> Result<Reply, String> {
        let v: J = serde_json::from_str(s).map_err(|e| format!("bad reply: {e}"))?;
        if v["ready"].as_bool() == Some(true) {
            return Ok(Reply::Ready);
        }
        if v["dead"].as_bool() == Some(true) {
            return Ok(Reply::Dead {
                error: v["error"].as_str().unwrap_or("worker died").to_owned(),
            });
        }
        if let Some(e) = v["bootError"].as_str() {
            return Ok(Reply::BootError {
                error: e.to_owned(),
            });
        }
        let seq = v["seq"].as_u64().ok_or("bad reply: no seq")?;
        let result = match v["ok"].as_bool() {
            Some(true) => {
                let mut stats = Stats::new();
                for (k, x) in v["stats"].as_object().into_iter().flatten() {
                    stats.insert(
                        k.clone(),
                        json_num(x).ok_or_else(|| format!("bad reply: stat {k}"))?,
                    );
                }
                Ok(stats)
            }
            Some(false) => Err(v["error"].as_str().unwrap_or("calc error").to_owned()),
            None => return Err("bad reply: no ok".to_owned()),
        };
        Ok(Reply::Done { seq, result })
    }
}

use protocol::{Reply, Request, Stats};

// ---- the worker side --------------------------------------------------------------------------

/// The numeric top-level entries of `output` (the cross-check's stats).
pub fn output_stats(output: &Output) -> Stats {
    output
        .pairs()
        .filter_map(|(k, v)| match (k, v) {
            (model::Key::S(name), model::Val::Num(x)) => Some((name.to_string(), x)),
            _ => None,
        })
        .collect()
}

/// The worker's calc for one request. A Lua error inside propagates (on wasm32 it aborts the
/// worker after its panic hook posted the error reply; [`ThreadHub`]'s threads catch it).
pub fn run_request(req: &Request) -> Reply {
    match req {
        Request::BuildOutput { seq, xml } => {
            // calc:BM-A: Build:Init + LoadDB up to Build.lua:694 (UpdateSocketGroups,
            // wipeGlobalCache), without the first BuildOutput
            let build = match model::build_xml::loadBuildModelFromXML(xml, model::data()) {
                Ok(b) => b,
                Err(e) => {
                    return Reply::Done {
                        seq: *seq,
                        result: Err(e),
                    }
                }
            };
            // the recalc of Build.lua:1260-1266, as the main thread runs it before build_output
            pob_calc::modules::common::wipeGlobalCache();
            build.skillsTab.UpdateSocketGroups(&build);
            power::BuildOutput(&build);
            let stats = build
                .calcsTab
                .mainOutput
                .get()
                .map(|o| output_stats(&o))
                .unwrap_or_default();
            // CCR-0142: the request's build is dropped here, but two `Rc` cycles would keep it
            // and its envs alive (Lua's GC collects them); `Build::teardown` breaks both and
            // frees the GlobalCache's envs (they point back to this build too).
            build.teardown();
            Reply::Done {
                seq: *seq,
                result: Ok(stats),
            }
        }
    }
}

/// [`run_request`] on a message: decodes, runs, encodes.
pub fn handle_message(msg: &str) -> String {
    match protocol::decode_request(msg) {
        Ok(req) => protocol::encode_reply(&run_request(&req)),
        Err(e) => protocol::encode_reply(&Reply::Done {
            seq: 0,
            result: Err(e),
        }),
    }
}

/// The tree versions loading `xml` reads (`<Spec treeVersion="…">`, plus the latest version every
/// build loads, build_xml.rs `latestTree`), restricted to `treeVersionList`. The web worker
/// fetches their files before the load (`tree_bytes` is synchronous).
pub fn tree_versions(xml: &str) -> Vec<String> {
    let d = model::data();
    let mut out = vec![d.gameVersions.latestTreeVersion.to_string()];
    let key = "treeVersion=\"";
    let mut rest = xml;
    while let Some(i) = rest.find(key) {
        rest = &rest[i + key.len()..];
        let Some(j) = rest.find('"') else { break };
        let v = &rest[..j];
        if !out.iter().any(|o| o == v)
            && d.gameVersions
                .treeVersionList
                .iter()
                .any(|t| t.as_str() == v)
        {
            out.push(v.to_owned());
        }
        rest = &rest[j..];
    }
    out
}

// ---- the client -------------------------------------------------------------------------------

/// What a worker reports to its client.
#[derive(Clone, Debug, PartialEq)]
pub enum WorkerEvent {
    /// A posted message (a protocol [`Reply`] as JSON).
    Message(String),
    /// The worker failed outside the protocol (a script error, `onerror`); it is dead.
    Error(String),
}

/// The client's end of one worker.
pub trait WorkerPort {
    fn post(&self, msg: &str);
    fn terminate(&self);
}

/// Where a worker's events go. Called outside any frame (a JS task on the web,
/// [`ThreadHub::pump`] on desktop), never from inside a [`WorkerPort`] call.
pub type Sink = Rc<dyn Fn(WorkerEvent)>;

/// Starts a worker that reports to the sink.
pub type Spawner = Rc<dyn Fn(Sink) -> Box<dyn WorkerPort>>;

/// The relative tolerance of the stats cross-check.
const STATS_REL_TOL: f64 = 1e-9;

/// Consecutive boot failures after which the client stops starting workers.
const MAX_BOOT_FAILURES: u32 = 2;

struct InFlight {
    seq: u64,
    xml: Rc<str>,
    /// Posted after this worker's error reply: the worker may be dying (its panic hook answered
    /// and the trap follows), so its death is not this request's verdict.
    after_error: bool,
}

struct Inner {
    spawn: Spawner,
    me: Weak<RefCell<Inner>>,
    port: Option<Box<dyn WorkerPort>>,
    /// Bumped per worker; events of an older worker are dropped.
    generation: u64,
    booted: bool,
    /// The current worker sent an error reply (a panic-hook reply is followed by its death).
    errored: bool,
    boot_failures: u32,
    next_seq: u64,
    in_flight: Option<InFlight>,
    queued: Option<Rc<str>>,
    /// The last answer: the XML it is for and the worker's result.
    verdict: Option<(Rc<str>, Result<Stats, String>)>,
    /// The XML already asked again after a worker died without answering it (one retry).
    retried: Option<Rc<str>>,
    /// The build whose `buildFlag` an answer raises.
    build: Weak<model::Build>,
    // status
    epoch: u64,
    requests: u64,
    restarts: u64,
    mismatches: u64,
    errors: Vec<String>,
    broken: Option<String>,
}

fn log_error(msg: &str) {
    #[cfg(target_arch = "wasm32")]
    web_sys::console::error_1(&wasm_bindgen::JsValue::from_str(msg));
    #[cfg(not(target_arch = "wasm32"))]
    eprintln!("{msg}");
}

impl Inner {
    fn ensure_port(&mut self) {
        if self.port.is_some() || self.broken.is_some() {
            return;
        }
        self.generation += 1;
        self.booted = false;
        self.errored = false;
        let generation = self.generation;
        let me = self.me.clone();
        let sink: Sink = Rc::new(move |ev| {
            if let Some(inner) = me.upgrade() {
                inner.borrow_mut().on_event(generation, ev);
            }
        });
        self.port = Some((self.spawn)(sink));
    }

    fn send(&mut self, xml: Rc<str>) {
        self.ensure_port();
        let Some(port) = &self.port else {
            return;
        };
        self.next_seq += 1;
        let seq = self.next_seq;
        port.post(&protocol::encode_request(&Request::BuildOutput {
            seq,
            xml: xml.to_string(),
        }));
        self.requests += 1;
        self.in_flight = Some(InFlight {
            seq,
            xml,
            after_error: self.errored,
        });
    }

    /// Ask the worker about `xml` (or remember it for when the current request is answered).
    fn request(&mut self, xml: Rc<str>) {
        match &self.in_flight {
            Some(f) if f.xml == xml => self.queued = None,
            Some(_) => self.queued = Some(xml),
            None => self.send(xml),
        }
    }

    fn raise_build_flag(&self) {
        if let Some(b) = self.build.upgrade() {
            b.buildFlag.set(true);
        }
    }

    fn report(&mut self, msg: String) {
        log_error(&format!("WorkerCalc: {msg}"));
        self.errors.push(msg);
    }

    fn on_event(&mut self, generation: u64, ev: WorkerEvent) {
        if generation != self.generation {
            return;
        }
        match ev {
            WorkerEvent::Message(s) => match protocol::decode_reply(&s) {
                Ok(Reply::Ready) => {
                    self.booted = true;
                    self.boot_failures = 0;
                }
                Ok(Reply::Done { seq, result }) => self.on_done(seq, result),
                Ok(Reply::Dead { error }) => self.on_dead(&error),
                Ok(Reply::BootError { error }) => {
                    self.report(format!("the calc worker did not start: {error}"));
                    self.booted = false;
                    self.on_dead(&error);
                }
                Err(e) => self.report(e),
            },
            WorkerEvent::Error(e) => self.on_dead(&e),
        }
    }

    fn on_done(&mut self, seq: u64, result: Result<Stats, String>) {
        let Some(f) = self.in_flight.take_if(|f| f.seq == seq) else {
            return;
        };
        if let Err(e) = &result {
            // Launch.lua:118 shows "In 'OnFrame': <msg>" (DIVERGENCE(worker-error))
            self.report(format!("calc error: {e}"));
            // a Lua error's reply comes from the panic hook and the worker dies next; an error
            // that did not raise (a bad XML) leaves it alive. The queued XML is posted either way
            // (a living worker answers it); `on_dead` asks it again of the next worker.
            self.errored = true;
        }
        let ok = result.is_ok();
        self.verdict = Some((f.xml, result));
        if let Some(next) = self.queued.take() {
            self.send(next);
        } else if ok {
            self.raise_build_flag();
        }
    }

    fn on_dead(&mut self, error: &str) {
        if let Some(port) = self.port.take() {
            port.terminate();
        }
        // events of this worker that are still queued are dropped from now on
        self.generation += 1;
        let booted = self.booted;
        if booted {
            self.boot_failures = 0;
        } else {
            self.boot_failures += 1;
        }
        self.restarts += 1;
        if let Some(f) = self.in_flight.take() {
            // The worker died without answering `f`. Its death is `f`'s verdict only when the
            // worker had booted, had not already answered with an error (then it was dying
            // when `f` was posted), and `f` was already asked again once (a crash that is not
            // the calc's, an out-of-memory say, is retried once). Otherwise `f` is asked of the
            // next worker, unless a newer XML waits.
            let retried = self.retried.as_ref().is_some_and(|r| *r == f.xml);
            if f.after_error || !booted || !retried {
                if booted && !f.after_error {
                    self.report(format!(
                        "the calc worker died without answering ({error}); asking a new worker"
                    ));
                    self.retried = Some(f.xml.clone());
                }
                if self.queued.is_none() {
                    self.queued = Some(f.xml);
                }
            } else {
                self.report(format!("calc error: {error}"));
                self.verdict = Some((f.xml, Err(error.to_owned())));
            }
        }
        if self.boot_failures >= MAX_BOOT_FAILURES {
            self.report(format!(
                "the calc worker failed to start {} times; calculating on the main thread",
                self.boot_failures
            ));
            self.broken = Some(error.to_owned());
            self.queued = None;
            self.raise_build_flag();
            return;
        }
        if let Some(next) = self.queued.take() {
            self.send(next);
        } else {
            // start the next worker now, so it boots before it is needed
            self.ensure_port();
        }
    }
}

/// A cloneable view of a [`WorkerCalc`]'s state (tests, the web host's status line).
#[derive(Clone)]
pub struct WorkerStatus(Rc<RefCell<Inner>>);

impl WorkerStatus {
    /// `Ready` results so far (the "result epoch" of ui doc §10.4).
    pub fn epoch(&self) -> u64 {
        self.0.borrow().epoch
    }
    /// Requests posted to workers.
    pub fn requests(&self) -> u64 {
        self.0.borrow().requests
    }
    /// Workers that died (and were replaced).
    pub fn restarts(&self) -> u64 {
        self.0.borrow().restarts
    }
    /// Stats where the live `mainOutput` differed from the worker's.
    pub fn mismatches(&self) -> u64 {
        self.0.borrow().mismatches
    }
    /// Every error reported so far (calc errors, worker failures, cross-check differences).
    pub fn errors(&self) -> Vec<String> {
        self.0.borrow().errors.clone()
    }
    /// A request is waiting for its answer.
    pub fn busy(&self) -> bool {
        self.0.borrow().in_flight.is_some()
    }
    /// The worker cannot start; the client calculates on the main thread.
    pub fn broken(&self) -> Option<String> {
        self.0.borrow().broken.clone()
    }
}

/// The wasm Web Worker client of the calc (module docs).
pub struct WorkerCalc {
    inner: Rc<RefCell<Inner>>,
}

impl WorkerCalc {
    /// A client whose workers `spawn` starts. The first worker starts now, so it boots (loads
    /// the game data) while the build's first calc runs on the main thread.
    pub fn new(spawn: Spawner) -> WorkerCalc {
        let inner = Rc::new_cyclic(|me| {
            RefCell::new(Inner {
                spawn,
                me: me.clone(),
                port: None,
                generation: 0,
                booted: false,
                errored: false,
                boot_failures: 0,
                next_seq: 0,
                in_flight: None,
                queued: None,
                verdict: None,
                retried: None,
                build: Weak::new(),
                epoch: 0,
                requests: 0,
                restarts: 0,
                mismatches: 0,
                errors: Vec::new(),
                broken: None,
            })
        });
        inner.borrow_mut().ensure_port();
        WorkerCalc { inner }
    }

    pub fn status(&self) -> WorkerStatus {
        WorkerStatus(self.inner.clone())
    }

    /// `calcsTab:BuildOutput()` on the live model (CalcsTab.lua:441-472, calc:PWR).
    fn run_on_main(&self, build: &BuildRef) {
        power::BuildOutput(build);
        self.inner.borrow_mut().epoch += 1;
    }

    /// Compare the live `mainOutput` with the worker's stats.
    fn cross_check(&self, build: &BuildRef, stats: &Stats) {
        let live = build
            .calcsTab
            .mainOutput
            .get()
            .map(|o| output_stats(&o))
            .unwrap_or_default();
        let mut diffs = Vec::new();
        for (k, w) in stats {
            let same = live.get(k).is_some_and(|m| {
                (m.is_nan() && w.is_nan())
                    || m == w
                    || (m - w).abs() <= STATS_REL_TOL * m.abs().max(w.abs())
            });
            if !same {
                diffs.push(format!("{k}: main {:?}, worker {w:?}", live.get(k)));
            }
        }
        for k in live.keys().filter(|k| !stats.contains_key(*k)) {
            diffs.push(format!("{k}: main {:?}, worker none", live[k]));
        }
        if !diffs.is_empty() {
            let mut inner = self.inner.borrow_mut();
            inner.mismatches += diffs.len() as u64;
            diffs.truncate(8);
            inner.report(format!(
                "the main thread's output differs from the worker's: {}",
                diffs.join("; ")
            ));
        }
    }
}

impl Drop for WorkerCalc {
    fn drop(&mut self) {
        if let Ok(mut inner) = self.inner.try_borrow_mut() {
            inner.generation += 1;
            if let Some(port) = inner.port.take() {
                port.terminate();
            }
        }
    }
}

impl CalcFacade for WorkerCalc {
    /// `calcsTab:BuildOutput()` (CalcsTab.lua:441-472) once the worker calculated the model's
    /// current XML without an error (module docs).
    fn build_output(&mut self, build: &BuildRef) -> Poll<()> {
        self.inner.borrow_mut().build = Rc::downgrade(build);
        // CCR-0134 §4: the build's first calc (Build.lua:695) has no Pending placeholder
        if build.calcsTab.mainEnv.get().is_none() || self.inner.borrow().broken.is_some() {
            self.run_on_main(build);
            return Poll::Ready(());
        }
        let xml: Rc<str> = build.SaveDB("").unwrap_or_default().into();
        let verdict = {
            let inner = self.inner.borrow();
            match &inner.verdict {
                Some((v, r)) if *v == xml => Some(r.clone()),
                _ => None,
            }
        };
        match verdict {
            Some(Ok(stats)) => {
                self.run_on_main(build);
                self.cross_check(build, &stats);
                // the page's console (debug level): which results came through the worker
                #[cfg(target_arch = "wasm32")]
                web_sys::console::debug_1(&wasm_bindgen::JsValue::from_str(&format!(
                    "WorkerCalc: result epoch {} (worker Life {:?})",
                    self.inner.borrow().epoch,
                    stats.get("Life")
                )));
                Poll::Ready(())
            }
            // the worker's calc raised for this model: keep the last results
            Some(Err(_)) => Poll::Pending,
            None => {
                self.inner.borrow_mut().request(xml);
                Poll::Pending
            }
        }
    }

    /// `calcsTab:GetMiscCalculator()` (CalcsTab.lua:760-762): the calculator the last `Ready`
    /// installed.
    fn misc_calculator(&mut self, build: &BuildRef) -> Poll<(Rc<dyn MiscCalc>, Output)> {
        if build.calcsTab.miscCalculator.get().is_none() {
            return Poll::Pending;
        }
        let (calc_func, calc_base) = power::GetMiscCalculator(build);
        Poll::Ready((Rc::new(RustMiscCalc(calc_func)), calc_base))
    }
}

// ---- desktop: worker threads ------------------------------------------------------------------

#[cfg(not(target_arch = "wasm32"))]
pub use thread::ThreadHub;

#[cfg(not(target_arch = "wasm32"))]
mod thread {
    use std::cell::Cell;
    use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
    use std::time::{Duration, Instant};

    use super::{protocol, Rc, RefCell, Reply, Sink, Spawner, WorkerEvent, WorkerPort};

    /// The calc's stack on a worker thread (the calc recurses deeply; pob-oracle's worker uses the
    /// same).
    const STACK: usize = 256 << 20;

    /// What a worker thread sends back: `(settles a request, event)`; [`ThreadHub::pump`] waits
    /// until every posted request is settled.
    type Out = (bool, WorkerEvent);

    struct Link {
        rx: Receiver<Out>,
        sink: Sink,
        outstanding: Rc<Cell<u64>>,
    }

    /// Desktop stand-in for the browser's workers: each worker is a thread with its own
    /// thread-locals (GlobalCache, DataSel) running [`super::run_request`]; a Lua error is caught,
    /// answered with the error reply and a `dead` message, and ends the thread, as the wasm
    /// worker's panic hook and trap do. Events reach the client only in [`ThreadHub::pump`]
    /// (the browser's event loop). Needs `pob_gamedata` initialised.
    #[derive(Clone, Default)]
    pub struct ThreadHub {
        links: Rc<RefCell<Vec<Link>>>,
    }

    struct ThreadPort {
        tx: RefCell<Option<Sender<String>>>,
        outstanding: Rc<Cell<u64>>,
    }

    impl WorkerPort for ThreadPort {
        fn post(&self, msg: &str) {
            if let Some(tx) = &*self.tx.borrow() {
                if tx.send(msg.to_owned()).is_ok() {
                    self.outstanding.set(self.outstanding.get() + 1);
                }
            }
        }
        fn terminate(&self) {
            // the thread ends after its current request (a browser worker stops at once)
            self.tx.borrow_mut().take();
            self.outstanding.set(0);
        }
    }

    fn worker_main(rx: &Receiver<String>, out: &Sender<Out>) {
        let send = |answer: bool, r: &Reply| {
            let _ = out.send((answer, WorkerEvent::Message(protocol::encode_reply(r))));
        };
        send(false, &Reply::Ready);
        for msg in rx {
            let req = match protocol::decode_request(&msg) {
                Ok(r) => r,
                Err(e) => {
                    send(
                        true,
                        &Reply::Done {
                            seq: 0,
                            result: Err(e),
                        },
                    );
                    continue;
                }
            };
            let seq = req.seq();
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                super::run_request(&req)
            })) {
                Ok(r) => send(true, &r),
                Err(p) => {
                    // the wasm worker's panic hook posts the error reply, then its script posts
                    // `dead` after the trap; the request is settled with the last of the two
                    let error = pob_host::panic_message(&*p);
                    send(
                        false,
                        &Reply::Done {
                            seq,
                            result: Err(error.clone()),
                        },
                    );
                    send(true, &Reply::Dead { error });
                    return;
                }
            }
        }
    }

    impl ThreadHub {
        pub fn new() -> ThreadHub {
            ThreadHub::default()
        }

        /// The spawner to give `WorkerCalc::new`.
        pub fn spawner(&self) -> Spawner {
            let links = self.links.clone();
            Rc::new(move |sink: Sink| {
                let (tx, rx) = channel::<String>();
                let (out_tx, out_rx) = channel::<Out>();
                let started = std::thread::Builder::new()
                    .name("pob-calc-worker".into())
                    .stack_size(STACK)
                    .spawn(move || worker_main(&rx, &out_tx));
                let outstanding = Rc::new(Cell::new(0));
                let port = ThreadPort {
                    tx: RefCell::new(Some(tx)),
                    outstanding: outstanding.clone(),
                };
                match started {
                    Ok(_) => links.borrow_mut().push(Link {
                        rx: out_rx,
                        sink,
                        outstanding,
                    }),
                    Err(e) => {
                        // delivered by the next pump, like a browser's error event
                        let (etx, erx) = channel::<Out>();
                        let _ = etx.send((false, WorkerEvent::Error(e.to_string())));
                        links.borrow_mut().push(Link {
                            rx: erx,
                            sink,
                            outstanding,
                        });
                    }
                }
                Box::new(port) as Box<dyn WorkerPort>
            })
        }

        /// Deliver the workers' events to their clients, waiting up to `timeout` until no posted
        /// request is unanswered. Returns the number of events delivered.
        pub fn pump(&self, timeout: Duration) -> usize {
            let deadline = Instant::now() + timeout;
            let mut delivered = 0;
            loop {
                // collect first: a sink may spawn a worker (which pushes a link)
                let mut events: Vec<(Sink, WorkerEvent)> = Vec::new();
                let mut waiting = false;
                {
                    let mut links = self.links.borrow_mut();
                    links.retain(|l| loop {
                        match l.rx.try_recv() {
                            Ok((answer, ev)) => {
                                if answer {
                                    l.outstanding.set(l.outstanding.get().saturating_sub(1));
                                }
                                events.push((l.sink.clone(), ev));
                            }
                            Err(std::sync::mpsc::TryRecvError::Empty) => {
                                waiting |= l.outstanding.get() > 0;
                                return true;
                            }
                            Err(std::sync::mpsc::TryRecvError::Disconnected) => return false,
                        }
                    });
                }
                delivered += events.len();
                let any = !events.is_empty();
                for (sink, ev) in events {
                    sink(ev);
                }
                if any {
                    continue;
                }
                let now = Instant::now();
                if !waiting || now >= deadline {
                    return delivered;
                }
                // block on the first link that owes an answer
                let links = self.links.borrow();
                if let Some(l) = links.iter().find(|l| l.outstanding.get() > 0) {
                    match l
                        .rx
                        .recv_timeout((deadline - now).min(Duration::from_millis(50)))
                    {
                        Ok((answer, ev)) => {
                            if answer {
                                l.outstanding.set(l.outstanding.get().saturating_sub(1));
                            }
                            let sink = l.sink.clone();
                            drop(links);
                            delivered += 1;
                            sink(ev);
                        }
                        Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => {}
                    }
                }
            }
        }
    }
}

// ---- wasm32: the browser's Web Worker ----------------------------------------------------------

#[cfg(target_arch = "wasm32")]
pub use web::{init_web_data, web_spawner, WebWorkerConfig, DEFAULT_DATA_BASE};

#[cfg(target_arch = "wasm32")]
mod web {
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    use wasm_bindgen::prelude::*;
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;

    use super::{protocol, Rc, Reply, Sink, Spawner, WorkerEvent, WorkerPort};

    /// Where the game data (`data.bin` and `trees/<name>.bin`) is served by default (relative to
    /// the page): the asset bundle's `gamedata/` (VENDOR).
    pub const DEFAULT_DATA_BASE: &str = "assets/gamedata";

    /// The worker's script. It is started from a Blob URL, so the module URLs it imports are
    /// absolute ([`WebWorkerConfig::resolve`]); they come in through the worker's `name`.
    /// Messages that arrive while the module loads are queued; requests run one at a time. A
    /// trap is reported as `dead` from `run`'s catch (the handler's promise rejected) or from the
    /// global error listeners (a trap thrown from a wasm-bindgen-futures task after the first
    /// `await` does not reject that promise); the page's `onerror` is the fallback route.
    const WORKER_JS: &str = r"
const cfg = JSON.parse(self.name);
const queue = [];
let handle = null;
let dead = false;
const die = (err) => {
  if (dead) return;
  dead = true;
  self.postMessage(JSON.stringify({ dead: true, error: String(err) }));
  self.close();
};
self.addEventListener('error', (e) => die(e.message || e.error));
self.addEventListener('unhandledrejection', (e) => die(e.reason));
self.onmessage = (e) => { if (handle) handle(e.data); else queue.push(e.data); };
(async () => {
  let m;
  try {
    m = await import(cfg.glue);
    await m.default();
    await m.pob_calc_worker_init(cfg.data);
  } catch (err) {
    self.postMessage(JSON.stringify({ bootError: String(err && err.stack || err) }));
    self.close();
    return;
  }
  let chain = Promise.resolve();
  const run = async (msg) => {
    if (dead) return;
    try {
      self.postMessage(await m.pob_calc_worker_handle(msg));
    } catch (err) {
      die(err);
    }
  };
  handle = (msg) => { chain = chain.then(() => run(msg)); };
  self.postMessage(JSON.stringify({ ready: true }));
  for (const msg of queue.splice(0)) handle(msg);
})();
";

    /// How the page's worker is started.
    #[derive(Clone, Debug, Default)]
    pub struct WebWorkerConfig {
        /// The wasm-bindgen glue of this app (`--target web`). `None`: the page's
        /// `<link rel="modulepreload" href="….js">` (Trunk adds one for the app), else the first
        /// `<script type="module" src>`, else `./pob-web.js`.
        pub glue_url: Option<String>,
        /// Where `data.bin` and `trees/` are served ([`DEFAULT_DATA_BASE`]).
        pub data_base: Option<String>,
    }

    impl WebWorkerConfig {
        /// The absolute `(glue, data)` URLs.
        fn resolve(&self) -> Result<(String, String), JsValue> {
            let doc = web_sys::window()
                .and_then(|w| w.document())
                .ok_or_else(|| JsValue::from_str("WorkerCalc: no document"))?;
            let base = doc.base_uri()?.unwrap_or_default();
            let found = || -> Option<String> {
                for sel in [
                    "link[rel=modulepreload][href$='.js']",
                    "script[type=module][src]",
                ] {
                    if let Ok(Some(e)) = doc.query_selector(sel) {
                        let attr = if sel.starts_with("link") {
                            "href"
                        } else {
                            "src"
                        };
                        if let Some(v) = e.get_attribute(attr) {
                            return Some(v);
                        }
                    }
                }
                None
            };
            let glue = self
                .glue_url
                .clone()
                .or_else(found)
                .unwrap_or_else(|| "./pob-web.js".to_owned());
            let data = self
                .data_base
                .clone()
                .unwrap_or_else(|| DEFAULT_DATA_BASE.to_owned());
            let abs = |u: &str| web_sys::Url::new_with_base(u, &base).map(|u| u.href());
            Ok((abs(&glue)?, abs(&data)?))
        }
    }

    struct WebPort {
        worker: web_sys::Worker,
        _onmessage: Closure<dyn FnMut(web_sys::MessageEvent)>,
        _onerror: Closure<dyn FnMut(web_sys::ErrorEvent)>,
    }

    impl WorkerPort for WebPort {
        fn post(&self, msg: &str) {
            let _ = self.worker.post_message(&JsValue::from_str(msg));
        }
        fn terminate(&self) {
            self.worker.terminate();
        }
    }

    impl Drop for WebPort {
        fn drop(&mut self) {
            self.worker.set_onmessage(None);
            self.worker.set_onerror(None);
        }
    }

    /// A port whose worker could not be created: it reports the error once, from a JS task.
    struct FailedPort;

    impl WorkerPort for FailedPort {
        fn post(&self, _: &str) {}
        fn terminate(&self) {}
    }

    fn start_worker(cfg: &WebWorkerConfig, sink: &Sink) -> Result<WebPort, JsValue> {
        let (glue, data) = cfg.resolve()?;
        let parts = js_sys::Array::of1(&JsValue::from_str(WORKER_JS));
        let bag = web_sys::BlobPropertyBag::new();
        bag.set_type("text/javascript");
        let blob = web_sys::Blob::new_with_str_sequence_and_options(&parts, &bag)?;
        let url = web_sys::Url::create_object_url_with_blob(&blob)?;
        let opts = web_sys::WorkerOptions::new();
        opts.set_type(web_sys::WorkerType::Module);
        opts.set_name(&serde_json::json!({ "glue": glue, "data": data }).to_string());
        let worker = web_sys::Worker::new_with_options(&url, &opts)?;
        let _ = web_sys::Url::revoke_object_url(&url);
        let s = sink.clone();
        let onmessage =
            Closure::<dyn FnMut(web_sys::MessageEvent)>::new(move |e: web_sys::MessageEvent| {
                let text = e.data().as_string().unwrap_or_default();
                s(WorkerEvent::Message(text));
            });
        let s = sink.clone();
        let onerror =
            Closure::<dyn FnMut(web_sys::ErrorEvent)>::new(move |e: web_sys::ErrorEvent| {
                s(WorkerEvent::Error(format!(
                    "{} ({}:{})",
                    e.message(),
                    e.filename(),
                    e.lineno()
                )));
            });
        worker.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
        worker.set_onerror(Some(onerror.as_ref().unchecked_ref()));
        Ok(WebPort {
            worker,
            _onmessage: onmessage,
            _onerror: onerror,
        })
    }

    /// The spawner of the page's calc workers.
    pub fn web_spawner(cfg: WebWorkerConfig) -> Spawner {
        Rc::new(move |sink: Sink| match start_worker(&cfg, &sink) {
            Ok(p) => Box::new(p) as Box<dyn WorkerPort>,
            Err(e) => {
                let msg = format!("{e:?}");
                // not from inside the spawner call (the client holds its state then)
                wasm_bindgen_futures::spawn_local(async move {
                    sink(WorkerEvent::Error(msg));
                });
                Box::new(FailedPort)
            }
        })
    }

    // ---- data on the web (both threads) ----

    /// `trees/<ver>.bin` bytes fetched so far (`pob_gamedata::tree_bytes` is synchronous).
    static TREES: Mutex<BTreeMap<String, Vec<u8>>> = Mutex::new(BTreeMap::new());
    static DATA_BASE: Mutex<String> = Mutex::new(String::new());

    fn tree_provider(ver: &str) -> Option<Vec<u8>> {
        TREES.lock().ok()?.get(ver).cloned()
    }

    async fn fetch_bytes(url: &str) -> Result<Vec<u8>, JsValue> {
        let global = js_sys::global();
        let p = if let Some(w) = global.dyn_ref::<web_sys::Window>() {
            w.fetch_with_str(url)
        } else {
            global
                .unchecked_into::<web_sys::WorkerGlobalScope>()
                .fetch_with_str(url)
        };
        let resp: web_sys::Response = JsFuture::from(p).await?.dyn_into()?;
        if !resp.ok() {
            return Err(JsValue::from_str(&format!("{url}: HTTP {}", resp.status())));
        }
        let buf = JsFuture::from(resp.array_buffer()?).await?;
        Ok(js_sys::Uint8Array::new(&buf).to_vec())
    }

    fn join(base: &str, rel: &str) -> String {
        format!("{}/{rel}", base.trim_end_matches('/'))
    }

    /// Fetch the tree files of `versions` that are not loaded yet (a missing file is skipped: the
    /// load then fails as it does on desktop without the file).
    pub async fn ensure_trees(versions: &[String]) {
        let base = DATA_BASE.lock().map(|b| b.clone()).unwrap_or_default();
        for v in versions {
            let have = TREES.lock().is_ok_and(|t| t.contains_key(v));
            if have {
                continue;
            }
            match fetch_bytes(&join(&base, &format!("trees/{v}.bin"))).await {
                Ok(b) => {
                    if let Ok(mut t) = TREES.lock() {
                        t.insert(v.clone(), b);
                    }
                }
                Err(e) => web_sys::console::warn_1(&e),
            }
        }
    }

    /// The game data on the web: fetch `<data_base>/data.bin`, `pob_gamedata::init`, serve the
    /// trees from [`TREES`] and fetch the latest tree version (the page: and the tree UI files
    /// of it). Both the page and its calc workers call it.
    pub async fn init_web_data(data_base: &str) -> Result<(), JsValue> {
        if let Ok(mut b) = DATA_BASE.lock() {
            data_base.clone_into(&mut b);
        }
        pob_gamedata::data::set_tree_provider(tree_provider);
        if !pob_gamedata::is_initialised() {
            let bytes = fetch_bytes(&join(data_base, "data.bin")).await?;
            pob_gamedata::init(&bytes).map_err(|e| JsValue::from_str(&format!("data.bin: {e}")))?;
        }
        let latest = pob_gamedata::data()
            .gameVersions
            .latestTreeVersion
            .to_string();
        if in_worker() {
            ensure_trees(&[latest]).await;
        } else {
            // VENDOR: the page's tree view also reads the tree UI files (sprites, the shared 3.19
            // asset table, the legion sprites; tree_view/assets.rs), which the calc never reads
            let sprites = pob_gamedata::data::tree_sprites_file(&latest);
            ensure_trees(&[
                latest,
                sprites,
                pob_gamedata::data::tree_assets_file("3_19"),
                pob_gamedata::data::TREE_LEGION_FILE.to_owned(),
            ])
            .await;
        }
        Ok(())
    }

    // ---- the worker's exports ----

    thread_local! {
        static CURRENT_SEQ: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    }

    fn post_from_worker(msg: &str) {
        if let Ok(scope) = js_sys::global().dyn_into::<web_sys::DedicatedWorkerGlobalScope>() {
            let _ = scope.post_message(&JsValue::from_str(msg));
        }
    }

    /// The worker's boot (its script calls it once): the panic hook that answers the current
    /// request with the Lua error before the abort, and the game data.
    #[wasm_bindgen]
    pub async fn pob_calc_worker_init(data_base: String) -> Result<(), JsValue> {
        std::panic::set_hook(Box::new(|info| {
            let error = pob_host::panic_message(info.payload());
            web_sys::console::error_1(&JsValue::from_str(&format!("calc worker: {info}")));
            let seq = CURRENT_SEQ.with(std::cell::Cell::get);
            post_from_worker(&protocol::encode_reply(&Reply::Done {
                seq,
                result: Err(error),
            }));
        }));
        init_web_data(&data_base).await
    }

    /// One request (its script calls it per message, one at a time): fetch the trees the build
    /// reads, then [`super::run_request`].
    #[wasm_bindgen]
    pub async fn pob_calc_worker_handle(msg: String) -> String {
        let req = match protocol::decode_request(&msg) {
            Ok(r) => r,
            Err(e) => {
                return protocol::encode_reply(&Reply::Done {
                    seq: 0,
                    result: Err(e),
                })
            }
        };
        let protocol::Request::BuildOutput { seq, xml } = &req;
        ensure_trees(&super::tree_versions(xml)).await;
        CURRENT_SEQ.with(|c| c.set(*seq));
        protocol::encode_reply(&super::run_request(&req))
    }

    /// Whether this module instance runs inside a worker (the app's `start` must do nothing
    /// there).
    pub fn in_worker() -> bool {
        js_sys::global()
            .dyn_ref::<web_sys::WorkerGlobalScope>()
            .is_some()
    }
}

#[cfg(target_arch = "wasm32")]
pub use web::in_worker;

#[cfg(test)]
mod tests {
    use super::protocol::*;

    #[test]
    fn protocol_round_trip() {
        let r = Request::BuildOutput {
            seq: 7,
            xml: "<PathOfBuilding>\"é\"</PathOfBuilding>".into(),
        };
        assert_eq!(decode_request(&encode_request(&r)), Ok(r));
        let mut stats = Stats::new();
        stats.insert("Life".into(), 5021.5);
        stats.insert("Nan".into(), f64::NAN);
        stats.insert("Inf".into(), f64::INFINITY);
        stats.insert("NegInf".into(), f64::NEG_INFINITY);
        stats.insert("NegZero".into(), -0.0);
        let back = decode_reply(&encode_reply(&Reply::Done {
            seq: 3,
            result: Ok(stats.clone()),
        }))
        .unwrap();
        let Reply::Done {
            seq: 3,
            result: Ok(b),
        } = back
        else {
            panic!("{back:?}")
        };
        assert_eq!(b.len(), stats.len());
        assert!(b["Nan"].is_nan());
        assert_eq!(b["Inf"], f64::INFINITY);
        assert_eq!(b["NegInf"], f64::NEG_INFINITY);
        assert_eq!(b["Life"], 5021.5);
        assert!(b["NegZero"] == 0.0 && b["NegZero"].is_sign_negative());
        for r in [
            Reply::Ready,
            Reply::Done {
                seq: 4,
                result: Err("attempt to index a nil value".into()),
            },
            Reply::Dead {
                error: "unreachable".into(),
            },
            Reply::BootError {
                error: "404".into(),
            },
        ] {
            assert_eq!(decode_reply(&encode_reply(&r)), Ok(r));
        }
        assert!(decode_request("{\"op\":\"x\",\"seq\":1}").is_err());
        assert!(decode_reply("{}").is_err());
    }
}
