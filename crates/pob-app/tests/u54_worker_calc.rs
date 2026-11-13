//! U54 acceptance (ui doc §12 U54 row): the web calc client `WorkerCalc` (calc doc D15).
//!
//! The browser's Web Worker is replaced by `ThreadHub`'s worker threads, which run the same
//! request handler as the wasm worker (`worker_web::run_request`) behind the same protocol; the
//! wasm glue itself is covered by `cargo check --target wasm32-unknown-unknown`.
//!
//! * `app_shows_numbers_once_the_worker_answers`: the real `PobApp` (Launch → Build mode, fonts
//!   from the PoB checkout) with a `WorkerCalc`: a fixture build opens with its numbers; after a
//!   model change the recalc is `Pending` (the sidebar keeps the old numbers) until the worker
//!   answers; the next frame shows the new numbers, equal to the worker's own calc.
//! * `facade_round_trip`: `build_output` / `misc_calculator` on a model: Pending, answer,
//!   buildFlag raised, Ready, the cached verdict, the stats cross-check.
//! * `client_state_machine`: a scripted port: calc error replies (kept Pending, reported), a dead
//!   worker (restart; the old worker's late events dropped), the one-slot queue, the cross-check
//!   catching a difference, two boot failures (main-thread fallback).
//! * `thread_worker_lua_error`: a request whose calc raises a Lua error on a worker thread: the
//!   error reply, the dead worker, the restart.
//!
//! Run with `--test-threads=1` (each test holds a build or two in memory).

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::task::Poll;
use std::time::Duration;

use pob_app::app::CxApp;
use pob_app::calc::worker_web::protocol::{self, Reply, Request, Stats};
use pob_app::calc::worker_web::{Sink, Spawner, ThreadHub, WorkerCalc, WorkerEvent, WorkerPort};
use pob_app::calc::CalcFacade;
use pob_calc::model::build_xml::loadBuildFromXML;
use pob_calc::model::BuildRef;
use pob_host::App;
use pob_sg::trace::{ApiCall, TraceRecorder};

const PUMP: Duration = Duration::from_secs(300);

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

fn life(build: &BuildRef) -> f64 {
    build
        .calcsTab
        .mainOutput
        .get()
        .expect("mainOutput")
        .num(pob_lua::Name::from("Life"))
}

// ---- the app --------------------------------------------------------------------------------

/// Every DrawString text of the last recorded frame.
fn last_frame_texts(app: &mut pob_app::PobApp) -> Vec<String> {
    let t = app.cx.sg.trace.as_ref().expect("trace");
    let f = t.frames.last().expect("a frame");
    f.calls
        .iter()
        .filter_map(|c| match &c.call {
            ApiCall::DrawString { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

/// `n` as FormatStat may print an integer stat (with and without thousands separators).
fn shown(texts: &[String], n: f64) -> bool {
    let plain = format!("{}", n.round() as i64);
    let mut sep = String::new();
    for (i, ch) in plain.chars().enumerate() {
        if i > 0 && (plain.len() - i) % 3 == 0 {
            sep.push(',');
        }
        sep.push(ch);
    }
    texts.iter().any(|t| t.contains(&plain) || t.contains(&sep))
}

#[test]
fn app_shows_numbers_once_the_worker_answers() {
    let Some(root) = root() else { return };
    let src = std::fs::canonicalize(root.join("src")).unwrap();
    let runtime = std::fs::canonicalize(root.join("runtime")).unwrap();
    let h =
        pob_host::headless::HeadlessState::new(&src.to_string_lossy(), &runtime.to_string_lossy());
    *h.user_path.borrow_mut() = format!("{}/", fixture_user().display());
    let (p, f, n) = pob_host::headless::services(&h);
    let mut parts = pob_host::headless::parts(Rc::new(pob_host::fs_native::DirAssets {
        src: src.clone(),
        runtime: runtime.join("SimpleGraphic"),
    }))
    .expect("fonts");
    parts.sys = pob_sg::Sys::new(p, f, n);

    let hub = ThreadHub::new();
    let statuses = Rc::new(RefCell::new(Vec::new()));
    let (spawn, st) = (hub.spawner(), statuses.clone());
    let mut app = pob_app::PobApp::with_calc(
        parts,
        Rc::new(move || {
            let wc = WorkerCalc::new(spawn.clone());
            st.borrow_mut().push(wc.status());
            Box::new(wc) as Box<dyn CalcFacade>
        }),
    );
    app.cx.sg.trace = Some(TraceRecorder::new());
    let screen = pob_sg::ScreenInfo {
        fb_w: 1280,
        fb_h: 800,
        win_h: 800,
        dpi_scale: 1.0,
    };
    let frame = |app: &mut pob_app::PobApp| {
        pob_host::run_frame(app, screen, (0, 0)).expect("frame");
        h.clock_ms.set(h.clock_ms.get() + 16);
    };
    pob_host::guarded(&mut app, App::on_init).expect("on_init");
    frame(&mut app);

    let xml = fixture_xml("allflame_exp_001.xml");
    pob_host::guarded(&mut app, |a| a.load_build("allflame_exp_001", &xml)).expect("load");
    for _ in 0..3 {
        frame(&mut app);
    }
    let model = app.cx.build().model.clone();
    let status = statuses.borrow().last().cloned().expect("a WorkerCalc");
    // the first calc (Build:Init) ran on the main thread (CCR-0134 §4)
    let life0 = life(&model);
    assert!(life0 > 0.0, "Life {life0}");
    assert!(
        shown(&last_frame_texts(&mut app), life0),
        "sidebar shows Life {life0}"
    );
    assert_eq!(status.requests(), 0);
    let epoch0 = status.epoch();

    // a model change: Build.lua:1258's recalc goes to the worker
    model.characterLevel.set(50.0);
    model.buildFlag.set(true);
    frame(&mut app);
    assert_eq!(status.requests(), 1, "one request posted");
    assert!(status.busy());
    assert_eq!(status.epoch(), epoch0, "Pending: no new results");
    assert_eq!(
        life(&model),
        life0,
        "the old results stay until the worker answers"
    );
    frame(&mut app);
    assert!(
        shown(&last_frame_texts(&mut app), life0),
        "the sidebar keeps the old numbers"
    );

    // the worker answers (the browser's event loop: between frames)
    assert!(hub.pump(PUMP) > 0, "the worker answered");
    assert!(!status.busy());
    assert!(model.buildFlag.get(), "the answer raises buildFlag");
    frame(&mut app);
    frame(&mut app);
    assert_eq!(status.epoch(), epoch0 + 1, "Ready once");
    let life1 = life(&model);
    assert!(life1 > 0.0 && life1 != life0, "Life {life0} -> {life1}");
    assert_eq!(
        status.mismatches(),
        0,
        "main thread == worker: {:?}",
        status.errors()
    );
    assert!(status.errors().is_empty(), "{:?}", status.errors());
    let texts = last_frame_texts(&mut app);
    assert!(
        shown(&texts, life1),
        "the sidebar shows the new Life {life1}"
    );
    assert_eq!(status.restarts(), 0);
    eprintln!(
        "u54 app: Life {life0} -> {life1}, requests {}, epoch {}, {} texts in the last frame",
        status.requests(),
        status.epoch(),
        texts.len()
    );
}

// ---- the facade on a model ------------------------------------------------------------------

#[test]
fn facade_round_trip() {
    if root().is_none() {
        return;
    }
    let hub = ThreadHub::new();
    let mut wc = WorkerCalc::new(hub.spawner());
    let status = wc.status();
    let build = loadBuildFromXML(
        &fixture_xml("upstream_Generals_Perforate_Zerker.xml"),
        pob_gamedata::data(),
    );
    let life0 = life(&build);
    assert!(matches!(wc.misc_calculator(&build), Poll::Ready(_)));

    build.characterLevel.set(60.0);
    build.buildFlag.set(false);
    assert_eq!(wc.build_output(&build), Poll::Pending);
    // asking again before the answer posts nothing new
    assert_eq!(wc.build_output(&build), Poll::Pending);
    assert_eq!(status.requests(), 1);
    assert_eq!(life(&build), life0);
    hub.pump(PUMP);
    assert!(build.buildFlag.get());
    assert_eq!(wc.build_output(&build), Poll::Ready(()));
    let life1 = life(&build);
    assert_ne!(life1, life0);
    assert_eq!(status.mismatches(), 0, "{:?}", status.errors());
    // the same model again: the verdict is known, no request
    assert_eq!(wc.build_output(&build), Poll::Ready(()));
    assert_eq!(status.requests(), 1);
    assert_eq!(status.epoch(), 2);
    let Poll::Ready((calc_func, base)) = wc.misc_calculator(&build) else {
        panic!("misc_calculator")
    };
    assert_eq!(base.num(pob_lua::Name::from("Life")), life1);
    let Poll::Ready(out) = calc_func.call(None, None) else {
        panic!("calcFunc")
    };
    assert_eq!(out.num(pob_lua::Name::from("Life")), life1);
    assert!(status.errors().is_empty(), "{:?}", status.errors());
    eprintln!("u54 facade: Life {life0} -> {life1}");
}

// ---- the state machine on a scripted port ---------------------------------------------------

#[derive(Default)]
struct Script {
    sinks: RefCell<Vec<Sink>>,
    posts: RefCell<Vec<String>>,
    terminated: Cell<u32>,
}

struct ScriptPort(Rc<Script>);

impl WorkerPort for ScriptPort {
    fn post(&self, msg: &str) {
        self.0.posts.borrow_mut().push(msg.to_owned());
    }
    fn terminate(&self) {
        self.0.terminated.set(self.0.terminated.get() + 1);
    }
}

fn scripted() -> (Rc<Script>, Spawner) {
    let s = Rc::new(Script::default());
    let s2 = s.clone();
    let spawn: Spawner = Rc::new(move |sink| {
        s2.sinks.borrow_mut().push(sink);
        Box::new(ScriptPort(s2.clone())) as Box<dyn WorkerPort>
    });
    (s, spawn)
}

fn send(s: &Script, worker: usize, r: &Reply) {
    let sink = s.sinks.borrow()[worker].clone();
    sink(WorkerEvent::Message(protocol::encode_reply(r)));
}

fn last_seq(s: &Script) -> u64 {
    let posts = s.posts.borrow();
    protocol::decode_request(posts.last().expect("a post"))
        .expect("request")
        .seq()
}

fn last_xml(s: &Script) -> String {
    let posts = s.posts.borrow();
    let Request::BuildOutput { xml, .. } =
        protocol::decode_request(posts.last().expect("a post")).expect("request");
    xml
}

#[test]
fn client_state_machine() {
    if root().is_none() {
        return;
    }
    let build = loadBuildFromXML(&fixture_xml("allflame_exp_048.xml"), pob_gamedata::data());
    let life0 = life(&build);

    let (s, spawn) = scripted();
    let mut wc = WorkerCalc::new(spawn);
    let status = wc.status();
    assert_eq!(s.sinks.borrow().len(), 1, "the first worker starts at once");
    send(&s, 0, &Reply::Ready);

    // a calc error: reported, the model keeps its results, no re-request for the same XML
    build.characterLevel.set(10.0);
    assert_eq!(wc.build_output(&build), Poll::Pending);
    let seq = last_seq(&s);
    send(
        &s,
        0,
        &Reply::Done {
            seq,
            result: Err("attempt to index a nil value".into()),
        },
    );
    assert_eq!(status.errors().len(), 1);
    assert!(status.errors()[0].contains("attempt to index a nil value"));
    assert_eq!(wc.build_output(&build), Poll::Pending);
    assert_eq!(s.posts.borrow().len(), 1);
    assert_eq!(life(&build), life0);

    // the worker then dies: terminated, replaced; its late events are dropped
    send(
        &s,
        0,
        &Reply::Dead {
            error: "RuntimeError: unreachable".into(),
        },
    );
    assert_eq!(s.terminated.get(), 1);
    assert_eq!(status.restarts(), 1);
    assert_eq!(s.sinks.borrow().len(), 2, "a new worker");
    send(
        &s,
        0,
        &Reply::Done {
            seq,
            result: Ok(Stats::new()),
        },
    );
    assert_eq!(wc.build_output(&build), Poll::Pending, "old worker ignored");

    // the one-slot queue: A in flight, B then C asked → C is sent after A's answer
    send(&s, 1, &Reply::Ready);
    build.characterLevel.set(11.0);
    assert_eq!(wc.build_output(&build), Poll::Pending);
    let (seq_a, xml_a) = (last_seq(&s), last_xml(&s));
    build.characterLevel.set(12.0);
    assert_eq!(wc.build_output(&build), Poll::Pending);
    build.characterLevel.set(13.0);
    assert_eq!(wc.build_output(&build), Poll::Pending);
    assert_eq!(s.posts.borrow().len(), 2, "B and C wait");
    build.buildFlag.set(false);
    send(
        &s,
        1,
        &Reply::Done {
            seq: seq_a,
            result: Ok(Stats::new()),
        },
    );
    assert_eq!(s.posts.borrow().len(), 3, "C sent");
    assert_ne!(last_xml(&s), xml_a);
    assert!(last_xml(&s).contains("level=\"13\""));
    assert!(!build.buildFlag.get(), "A's answer is stale: no recalc");
    // C answered with stats that differ from the live calc: Ready, and the cross-check reports it
    let mut stats = Stats::new();
    stats.insert("Life".into(), -1.0);
    send(
        &s,
        1,
        &Reply::Done {
            seq: last_seq(&s),
            result: Ok(stats),
        },
    );
    assert!(build.buildFlag.get());
    assert_eq!(wc.build_output(&build), Poll::Ready(()));
    assert!(status.mismatches() > 0);
    assert!(status.errors().last().unwrap().contains("Life"));

    // a queued XML is not lost to a dying worker: A raises (the panic hook's reply, then the
    // death) while B waits; B is posted to the old worker (a worker that survives an error that
    // did not raise answers it), and asked again of the new one when the old one dies
    let (s, spawn) = scripted();
    let mut wc2 = WorkerCalc::new(spawn);
    let st = wc2.status();
    send(&s, 0, &Reply::Ready);
    build.characterLevel.set(30.0);
    assert_eq!(wc2.build_output(&build), Poll::Pending);
    let seq_a = last_seq(&s);
    build.characterLevel.set(31.0);
    assert_eq!(wc2.build_output(&build), Poll::Pending);
    assert_eq!(s.posts.borrow().len(), 1, "B waits");
    send(
        &s,
        0,
        &Reply::Done {
            seq: seq_a,
            result: Err("boom".into()),
        },
    );
    assert_eq!(s.posts.borrow().len(), 2);
    assert!(last_xml(&s).contains("level=\"31\""));
    send(
        &s,
        0,
        &Reply::Dead {
            error: "RuntimeError: unreachable".into(),
        },
    );
    assert_eq!(s.sinks.borrow().len(), 2, "a new worker");
    assert_eq!(s.posts.borrow().len(), 3, "B asked again of the new worker");
    assert!(last_xml(&s).contains("level=\"31\""));
    assert!(st.busy());
    assert_eq!(
        st.errors(),
        vec!["calc error: boom".to_owned()],
        "A's error only"
    );
    send(&s, 1, &Reply::Ready);
    assert_eq!(
        wc2.build_output(&build),
        Poll::Pending,
        "B is being calculated"
    );
    assert_eq!(s.posts.borrow().len(), 3, "not asked twice");
    build.buildFlag.set(false);
    let mut stats_b = Stats::new();
    stats_b.insert("Life".into(), 1.0);
    send(
        &s,
        1,
        &Reply::Done {
            seq: last_seq(&s),
            result: Ok(stats_b),
        },
    );
    assert!(build.buildFlag.get(), "B's answer recalculates");
    assert_eq!(wc2.build_output(&build), Poll::Ready(()));

    // a death without an answer (not a calc error: out of memory, say) is retried once; the
    // second one is the model's verdict
    build.characterLevel.set(32.0);
    assert_eq!(wc2.build_output(&build), Poll::Pending);
    assert_eq!(s.posts.borrow().len(), 4);
    send(
        &s,
        1,
        &Reply::Dead {
            error: "out of memory".into(),
        },
    );
    assert_eq!(s.sinks.borrow().len(), 3);
    assert_eq!(s.posts.borrow().len(), 5, "retried");
    assert!(last_xml(&s).contains("level=\"32\""));
    send(&s, 2, &Reply::Ready);
    send(
        &s,
        2,
        &Reply::Dead {
            error: "out of memory".into(),
        },
    );
    assert_eq!(s.sinks.borrow().len(), 4);
    assert!(!st.busy());
    assert_eq!(
        wc2.build_output(&build),
        Poll::Pending,
        "the verdict: keep the last results"
    );
    assert_eq!(s.posts.borrow().len(), 5, "not retried twice");
    assert!(st
        .errors()
        .last()
        .unwrap()
        .contains("calc error: out of memory"));

    // a worker that dies before it booted does not decide the request: the next worker gets it
    build.characterLevel.set(33.0);
    assert_eq!(wc2.build_output(&build), Poll::Pending);
    assert_eq!(
        s.posts.borrow().len(),
        6,
        "posted before the boot (the script queues it)"
    );
    send(
        &s,
        3,
        &Reply::BootError {
            error: "404".into(),
        },
    );
    assert!(st.broken().is_none());
    assert_eq!(s.sinks.borrow().len(), 5);
    assert_eq!(s.posts.borrow().len(), 7);
    assert!(last_xml(&s).contains("level=\"33\""));
    send(&s, 4, &Reply::Ready);
    send(
        &s,
        4,
        &Reply::Done {
            seq: last_seq(&s),
            result: Ok(Stats::new()),
        },
    );
    assert_eq!(wc2.build_output(&build), Poll::Ready(()));
    assert_eq!(st.restarts(), 4);
    drop(wc2);

    // two boot failures: the client calculates on the main thread
    let (s, spawn) = scripted();
    let mut wc = WorkerCalc::new(spawn);
    let status = wc.status();
    send(
        &s,
        0,
        &Reply::BootError {
            error: "404".into(),
        },
    );
    assert!(status.broken().is_none());
    send(
        &s,
        1,
        &Reply::BootError {
            error: "404".into(),
        },
    );
    assert!(status.broken().is_some());
    assert_eq!(s.sinks.borrow().len(), 2, "no third worker");
    build.characterLevel.set(20.0);
    let epoch = status.epoch();
    assert_eq!(wc.build_output(&build), Poll::Ready(()));
    assert_eq!(status.epoch(), epoch + 1);
    assert_eq!(status.requests(), 0);
}

// ---- a Lua error on a worker thread ---------------------------------------------------------

/// An empty build XML: `build:LoadDB("")` raises "attempt to index field '?' (a nil value)" in
/// calc:BM-A (as PoB's `dbXML[1]` index does on an empty parse).
const RAISING_XML: &str = "";

/// A port that replaces the posted XML with [`RAISING_XML`] while `poison` is set (a model whose
/// calc raises in the worker).
struct Poisoned {
    inner: Box<dyn WorkerPort>,
    poison: Rc<Cell<bool>>,
}

impl WorkerPort for Poisoned {
    fn post(&self, msg: &str) {
        if self.poison.get() {
            let seq = protocol::decode_request(msg).expect("request").seq();
            self.inner
                .post(&protocol::encode_request(&Request::BuildOutput {
                    seq,
                    xml: RAISING_XML.into(),
                }));
        } else {
            self.inner.post(msg);
        }
    }
    fn terminate(&self) {
        self.inner.terminate();
    }
}

#[test]
fn thread_worker_lua_error() {
    if root().is_none() {
        return;
    }
    let raised = std::panic::catch_unwind(|| {
        pob_app::calc::worker_web::run_request(&Request::BuildOutput {
            seq: 1,
            xml: RAISING_XML.into(),
        })
    });
    assert!(raised.is_err(), "the empty XML raises a Lua error");

    // the hub alone: Ready, the error reply, dead
    let hub = ThreadHub::new();
    let got = Rc::new(RefCell::new(Vec::<WorkerEvent>::new()));
    let g = got.clone();
    let port = (hub.spawner())(Rc::new(move |ev| g.borrow_mut().push(ev)));
    port.post(&protocol::encode_request(&Request::BuildOutput {
        seq: 9,
        xml: RAISING_XML.into(),
    }));
    hub.pump(PUMP);
    let replies: Vec<Reply> = got
        .borrow()
        .iter()
        .map(|e| match e {
            WorkerEvent::Message(m) => protocol::decode_reply(m).expect("reply"),
            WorkerEvent::Error(e) => panic!("error event {e}"),
        })
        .collect();
    assert_eq!(replies.len(), 3, "{replies:?}");
    assert_eq!(replies[0], Reply::Ready);
    assert!(
        matches!(&replies[1], Reply::Done { seq: 9, result: Err(e) } if e.contains("nil value")),
        "{replies:?}"
    );
    assert!(matches!(&replies[2], Reply::Dead { .. }), "{replies:?}");

    // the client: the error keeps the model's results, the worker is replaced, and the next
    // model is calculated by the new worker
    let poison = Rc::new(Cell::new(true));
    let (p2, hub_spawn) = (poison.clone(), hub.spawner());
    let spawn: Spawner = Rc::new(move |sink| {
        Box::new(Poisoned {
            inner: hub_spawn(sink),
            poison: p2.clone(),
        }) as Box<dyn WorkerPort>
    });
    let mut wc = WorkerCalc::new(spawn);
    let status = wc.status();
    let build = loadBuildFromXML(&fixture_xml("allflame_exp_030.xml"), pob_gamedata::data());
    let life0 = life(&build);
    build.characterLevel.set(40.0);
    assert_eq!(wc.build_output(&build), Poll::Pending);
    hub.pump(PUMP);
    assert_eq!(status.restarts(), 1, "{:?}", status.errors());
    assert!(
        status.errors().iter().any(|e| e.contains("nil value")),
        "{:?}",
        status.errors()
    );
    assert!(!build.buildFlag.get(), "an error does not recalculate");
    assert_eq!(
        wc.build_output(&build),
        Poll::Pending,
        "the failed model stays Pending"
    );
    assert_eq!(life(&build), life0, "the last good results stay");

    poison.set(false);
    build.characterLevel.set(41.0);
    assert_eq!(wc.build_output(&build), Poll::Pending);
    hub.pump(PUMP);
    assert!(build.buildFlag.get());
    assert_eq!(wc.build_output(&build), Poll::Ready(()));
    assert_ne!(life(&build), life0);
    assert_eq!(status.mismatches(), 0, "{:?}", status.errors());
    assert_eq!(status.requests(), 2);

    // a model queued behind the raising one is calculated by the next worker (the old thread
    // returned after its `dead`, so the post of the queued XML to it is lost)
    poison.set(true);
    build.characterLevel.set(42.0);
    assert_eq!(wc.build_output(&build), Poll::Pending);
    poison.set(false);
    build.characterLevel.set(43.0);
    assert_eq!(wc.build_output(&build), Poll::Pending);
    build.buildFlag.set(false);
    hub.pump(PUMP);
    assert_eq!(status.restarts(), 2, "{:?}", status.errors());
    assert!(
        build.buildFlag.get(),
        "the queued model is answered: {:?}",
        status.errors()
    );
    let life2 = life(&build);
    assert_eq!(wc.build_output(&build), Poll::Ready(()));
    assert_ne!(life(&build), life2);
    assert_eq!(status.mismatches(), 0, "{:?}", status.errors());
    assert_eq!(
        status
            .errors()
            .iter()
            .filter(|e| e.contains("calc error"))
            .count(),
        2,
        "one error per raising model: {:?}",
        status.errors()
    );
    eprintln!(
        "u54 thread worker: {:?}; errors {:?}",
        replies[1],
        status.errors()
    );
}
