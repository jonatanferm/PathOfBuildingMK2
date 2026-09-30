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
    /// Run read-only Lua and return its first result as a string, without
    /// recalculating or snapshotting ([`Event::QueryResult`]).
    Query { code: String },
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
    /// PoB's sidebar, exactly as its stat box shows it (PoB colour codes kept).
    pub sidebar: Vec<SidebarLine>,
    /// PoB's warnings (e.g. insufficient mana, unmet requirements).
    pub warnings: Vec<String>,
    /// PoB's passive point counter (`^7123 / 122   ^78 / 8`) and its tooltip.
    pub point_display: String,
    pub point_req: String,
    /// JSON array of the sidebar's main-skill controls (`__mk2_ctl`).
    pub skill_select: String,
    /// JSON of PoB's specs / item / skill / config sets (`__mk2_sets`).
    pub sets: String,
}

/// One line of PoB's sidebar: `label` / `value` with `^7` / `^xRRGGBB`
/// colour codes; `height` 6 = spacer.
#[derive(Debug, Clone, Default)]
pub struct SidebarLine {
    pub label: String,
    pub value: String,
    pub height: f32,
}

/// Messages from the worker.
#[derive(Debug, Clone)]
pub enum Event {
    Booted {
        millis: u128,
    },
    BootFailed(String),
    Snapshot(Snapshot),
    QueryResult {
        seq: u64,
        result: Result<String, String>,
    },
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
        if let Command::Query { code } = &cmd {
            let result = pob
                .exec(code)
                .map(|v| v.first().map(|x| x.to_string()).unwrap_or_default());
            if tx.send(Event::QueryResult { seq, result }).is_err() {
                return;
            }
            continue;
        }
        let mut batch = vec![(seq, cmd)];
        while let Ok(next) = rx.try_recv() {
            if let Command::Query { code } = &next.1 {
                // Queries see the state before the rest of this batch; answer
                // them in order after applying what came before.
                let _ = code;
                batch.push(next);
                break;
            }
            batch.push(next);
        }
        let mut last_seq = seq;
        let mut error = None;
        let mut result = None;
        let mut deferred_query = None;
        for (s, cmd) in batch {
            if let Command::Query { code } = cmd {
                deferred_query = Some((s, code));
                continue;
            }
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
        if let Some((qseq, code)) = deferred_query {
            let result = pob
                .exec(&code)
                .map(|v| v.first().map(|x| x.to_string()).unwrap_or_default());
            if tx.send(Event::QueryResult { seq: qseq, result }).is_err() {
                return;
            }
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
        Command::Refresh | Command::Query { .. } => Ok(None),
    }
}

fn snapshot(pob: &mut Pob, seq: u64, result: Option<String>, error: Option<String>) -> Snapshot {
    let xml = pob
        .exec("return build and build:SaveDB('code') or ''")
        .ok()
        .and_then(|v| v.first().and_then(|x| x.as_str().map(str::to_owned)))
        .unwrap_or_default();
    let outputs = pob.player_output().unwrap_or_default();
    let ret = pob
        .exec(
            r"
local out = {}
local list = build and build.controls.statBox and build.controls.statBox.list or {}
for _, e in ipairs(list) do
    out[#out + 1] = tostring(e.height or 16) .. '\t' .. tostring(e[1] or '') .. '\t' .. tostring(e[2] or '')
end
out[#out + 1] = '\1'
for _, w in ipairs(build and build.controls.warnings and build.controls.warnings.lines or {}) do
    out[#out + 1] = tostring(w)
end
local pd = build and build.controls.pointDisplay
local ctls = {}
if build then
    pcall(function() build:RefreshSkillSelectControls(build.controls, build.mainSocketGroup, '') end)
    for _, n in ipairs({ 'mainSocketGroup', 'mainSkill', 'mainSkillPart', 'mainSkillStageCount', 'mainSkillMineCount', 'mainSkillMinion', 'mainSkillMinionSkill' }) do
        local c = build.controls[n]
        if c then ctls[#ctls + 1] = __mk2_ctl(n, c) end
    end
end
local sets = '{}'
if build then
    local ok, v = pcall(__mk2_sets)
    if ok then sets = __mk2_json(v) end
end
return table.concat(out, '\n'), pd and pd.str or '', pd and pd.req or '', __mk2_json(ctls), sets
",
        )
        .unwrap_or_default();
    let nth = |i: usize| ret.get(i).map(ToString::to_string).unwrap_or_default();
    let raw = nth(0);
    let (lines, warns) = raw.split_once("\n\u{1}").unwrap_or((raw.as_str(), ""));
    let sidebar = lines
        .lines()
        .map(|l| {
            let mut parts = l.splitn(3, '\t');
            SidebarLine {
                height: parts.next().and_then(|h| h.parse().ok()).unwrap_or(16.0),
                label: parts.next().unwrap_or("").to_owned(),
                value: parts.next().unwrap_or("").to_owned(),
            }
        })
        .collect();
    let warnings = warns
        .lines()
        .filter(|l| !l.is_empty())
        .map(str::to_owned)
        .collect();
    Snapshot {
        seq,
        xml,
        outputs,
        result,
        error,
        sidebar,
        warnings,
        point_display: nth(1),
        point_req: nth(2),
        skill_select: nth(3),
        sets: nth(4),
    }
}
