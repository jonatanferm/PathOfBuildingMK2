//! A background thread that owns a booted [`Pob`] and serves commands.
//!
//! The interpreter isn't `Send`, so it lives entirely on the worker thread;
//! the UI talks to it with plain messages and receives [`Snapshot`]s — the
//! build's PoB XML plus calc outputs — after every command. Recalculation is
//! PoB's own: mutations set `build.buildFlag` and the next `OnFrame` runs
//! `calcsTab:BuildOutput()`, exactly as in the desktop app.

use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::thread::JoinHandle;

use crate::headless::{Outputs, Pob};
use crate::lua::host::StdFs;
use crate::lua::Val;

/// A request to the worker.
#[derive(Debug, Clone)]
pub enum Command {
    /// Load a build from PoB XML (replaces the current build).
    LoadXml { xml: String, name: String },
    /// Start a new empty build.
    NewBuild,
    /// Run Lua against the loaded build (`build` is the build mode object),
    /// then let PoB recalculate. The chunk's first return value (if a
    /// string) comes back in [`Snapshot::result`].
    Lua { code: String },
    /// Just resend the current snapshot.
    Refresh,
}

/// State after a command.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    /// Sequence number of the command this answers.
    pub seq: u64,
    pub xml: String,
    pub outputs: Outputs,
    pub result: Option<String>,
    pub error: Option<String>,
}

/// Messages from the worker.
#[derive(Debug, Clone)]
pub enum Event {
    Booted { millis: u128 },
    BootFailed(String),
    Snapshot(Snapshot),
}

/// UI-side handle.
pub struct PobHandle {
    tx: Sender<(u64, Command)>,
    rx: Receiver<Event>,
    next_seq: u64,
    _thread: JoinHandle<()>,
}

impl PobHandle {
    /// Spawn the worker; PoB's `src/` is read from `pob_src`.
    #[must_use]
    pub fn spawn(pob_src: PathBuf) -> Self {
        let (tx, worker_rx) = channel::<(u64, Command)>();
        let (worker_tx, rx) = channel::<Event>();
        let thread = std::thread::Builder::new()
            .name("pob-lua".into())
            // Deep PoB call chains recurse through the tree-walking evaluator.
            .stack_size(256 * 1024 * 1024)
            .spawn(move || run(pob_src, &worker_rx, &worker_tx))
            .expect("spawn pob worker");
        Self {
            tx,
            rx,
            next_seq: 1,
            _thread: thread,
        }
    }

    /// Queue a command; returns its sequence number.
    pub fn send(&mut self, cmd: Command) -> u64 {
        let seq = self.next_seq;
        self.next_seq += 1;
        let _ = self.tx.send((seq, cmd));
        seq
    }

    /// Next pending event, if any (non-blocking).
    pub fn try_recv(&self) -> Option<Event> {
        match self.rx.try_recv() {
            Ok(e) => Some(e),
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => None,
        }
    }

    /// Block until the next event (tools / tests).
    pub fn recv(&self) -> Option<Event> {
        self.rx.recv().ok()
    }
}

fn run(pob_src: PathBuf, rx: &Receiver<(u64, Command)>, tx: &Sender<Event>) {
    let t = std::time::Instant::now();
    let mut pob = match Pob::boot(Rc::new(StdFs { root: pob_src })) {
        Ok(p) => p,
        Err(e) => {
            let _ = tx.send(Event::BootFailed(e));
            return;
        }
    };
    let _ = tx.send(Event::Booted {
        millis: t.elapsed().as_millis(),
    });
    while let Ok((seq, cmd)) = rx.recv() {
        // Coalesce: if more commands are queued, run them all before
        // snapshotting (only the last snapshot matters to the UI).
        let mut batch = vec![(seq, cmd)];
        while let Ok(next) = rx.try_recv() {
            batch.push(next);
        }
        let mut last_seq = seq;
        let mut error = None;
        let mut result = None;
        for (s, cmd) in batch {
            last_seq = s;
            match apply(&mut pob, cmd) {
                Ok(r) => {
                    if r.is_some() {
                        result = r;
                    }
                }
                Err(e) => error = Some(e),
            }
        }
        let snapshot = snapshot(&mut pob, last_seq, result, error);
        if tx.send(Event::Snapshot(snapshot)).is_err() {
            return;
        }
    }
}

fn apply(pob: &mut Pob, cmd: Command) -> Result<Option<String>, String> {
    match cmd {
        Command::LoadXml { xml, name } => pob.load_build_xml(&xml, &name).map(|()| None),
        Command::NewBuild => pob
            .exec(
                r#"
__mainObject__.promptMsg = nil
__mainObject__.main:SetMode("BUILD", false, "New build")
runCallback("OnFrame")
build = __mainObject__.main.modes["BUILD"]
"#,
            )
            .map(|_| None),
        Command::Lua { code } => {
            let ret = pob.exec(&code)?;
            pob.exec(
                r"
build.buildFlag = true
runCallback('OnFrame')
if __mainObject__.promptMsg then
    local msg = __mainObject__.promptMsg
    __mainObject__.promptMsg = nil
    error(msg)
end
",
            )?;
            Ok(ret.first().and_then(|v| match v {
                Val::Str(s) => Some(s.to_str().into_owned()),
                _ => None,
            }))
        }
        Command::Refresh => Ok(None),
    }
}

fn snapshot(pob: &mut Pob, seq: u64, result: Option<String>, error: Option<String>) -> Snapshot {
    let xml = pob
        .exec("return build and build:SaveDB('code') or ''")
        .ok()
        .and_then(|v| v.first().and_then(|x| x.as_str().map(str::to_owned)))
        .unwrap_or_default();
    let outputs = pob.player_output().unwrap_or_default();
    Snapshot {
        seq,
        xml,
        outputs,
        result,
        error,
    }
}
