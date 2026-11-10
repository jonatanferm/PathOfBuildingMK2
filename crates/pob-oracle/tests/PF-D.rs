//! Unit PF-D acceptance (calc doc §11.1 row PF-D, §9.4 L4): CalcPerform.lua 1943-2307
//! (`pob_calc::modules::calc_perform::pools_charges::seg`).
//!
//! L4 `perform:1943` → `perform:2308`: the main env's `perform:1943` dump is injected, the Rust
//! segment runs, and the state (env + perform locals) is diffed against PoB's `perform:2308` dump.
//! The callees other units port (`doActorAttribsConditions`, `doActorLifeMana`,
//! `doActorLifeManaReservation`, `getCachedOutputValue` (PF-A), `addMinionModifiers`,
//! `doActorCharges` (PF-B), `mergeTinctures` (PF-C)) are replayed (calc doc §9.2 "Replay"): the
//! segment calls them through `pools_charges::callee_replay` (CCR-0093), and this test answers each
//! call by applying the state delta PoB recorded around that call site (`pob_oracle::replay`).
//! The extra line probes around the call sites are in `corpus/PF-D/stages.toml` (`pfd:<line>`
//! fires before `<line>`); fill them with
//!
//! ```text
//! scripts/port/oracle.sh run --build .parity/builds/X.xml --stages crates/pob-oracle/corpus/PF-D/stages.toml \
//!     --stage 'perform:1943@main,perform:2308@main,pfd:*@main,getCachedOutputValue:*@main'
//! ```
//!
//! (`corpus/PF-D/probe_all.sh` does it for every all57 build and the unit corpus.) All dumps of a
//! build are read from the one probe-cache directory that holds its `pfd:*` dumps, so `:in` and
//! `:out` come from the same PoB process.
//!
//! A build that calls `getCachedOutputValue` (CalcPerform.lua:2281/2288/2299) has a replay floor:
//! replaying that callee alone (no PF-D code) already leaves identity/aliasing mismatches (its new
//! CALCULATOR env shares gem instances and effects with this one, and the replay copies tables in),
//! so those exact mismatches are excluded and counted (`replay_alone`).
//!
//! Unit corpus: `pfd_spectres_blight.xml` (allflame_exp_011 plus four `<Spectre>`s, Raise Spectres
//! picking them and Blight with skillPart 2): the spectre loop (MinionModifier / AllyModifier /
//! EnemyCurseLimit, HaveBeastSpectre) and the Blight stage mods.
//!
//! Gates (§9.4 L4): every unit-corpus build (`corpus/PF-D/*.xml`) passes; ≥ 98 % of the all57
//! builds that have dumps pass. Non-vacuity: the injected `perform:1943` state differs from
//! `perform:2308` on every build (the "before" count is printed), and every recorded call site the
//! segment reached was replayed (an unreplayed callee shows up as a stub hit and fails the build).
//!
//! Knobs: `PFD_BUILDS=stem1,stem2`, `PFD_VERBOSE=1` (every mismatch). Missing inputs skip with a
//! message; with `POB_ORACLE_REQUIRE=1` (or `CI`) they fail.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use pob_calc::modules::calc_perform::pools_charges::callee_replay;
use pob_calc::Val;
use pob_oracle::diff::Mismatch;
use pob_oracle::oracle::find_parity_dir;
use pob_oracle::replay::{Call, Replay};
use pob_oracle::schema::StateDump;
use pob_oracle::testkit;

/// The unported-stub registry (`pob_calc::stub`) is process-global, so the two tests must not
/// overlap: one test's `load_build` (BM-A's `LoadDB` stub) would land in the other's
/// `stub_reset` .. `stub_report` window and fail it as an unreplayed callee.
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn skip(why: &str) {
    let required = std::env::var_os("POB_ORACLE_REQUIRE").is_some_and(|v| v == "1")
        || std::env::var_os("CI").is_some_and(|v| !v.is_empty() && v != "false" && v != "0");
    assert!(!required, "required input missing: {why}");
    eprintln!("SKIPPED: {why}");
}

/// Call site (as `pools_charges` names it) → the probes PoB fired just before and after it.
const SITES: &[(&str, &str, &str)] = &[
    ("pfd:1944", "perform:1943", "pfd:1945"), // doActorAttribsConditions(env, env.player)
    ("pfd:1945", "pfd:1945", "pfd:1946"),     // doActorLifeMana(env.player)
    ("pfd:1957", "pfd:1957", "pfd:1958"),     // addMinionModifiers(…, env.minion)
    ("pfd:1964", "pfd:1964", "pfd:1965"),     // doActorAttribsConditions(env, env.minion)
    ("pfd:2092", "pfd:2092", "pfd:2094"),     // doActorLifeManaReservation(env.player, …)
    ("pfd:2095", "pfd:2095", "pfd:2096"),     // mergeTinctures(env.tinctures)
    ("pfd:2191", "pfd:2191", "pfd:2194"),     // doActorCharges(env, env.player)
    (
        "getCachedOutputValue",
        "getCachedOutputValue:in",
        "getCachedOutputValue:out",
    ),
];

fn stem(p: &Path) -> &str {
    p.file_stem().and_then(|s| s.to_str()).unwrap_or_default()
}

fn xmls_in(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|r| {
            r.filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "xml"))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    if let Ok(list) = std::env::var("PFD_BUILDS") {
        let want: Vec<&str> = list.split(',').map(str::trim).collect();
        v.retain(|p| want.contains(&stem(p)));
    }
    v
}

fn unit_corpus() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("corpus/PF-D")
}

/// The probe-cache directory with this build's `pfd:*` dumps (one PoB process).
fn pfd_dir(xml: &Path) -> Option<PathBuf> {
    testkit::dump_dirs(stem(xml), Some(xml))
        .into_iter()
        .find(|d| testkit::find_dump(std::slice::from_ref(d), "pfd:2194").is_some())
}

fn load(dir: &Path, hit: &str) -> Option<StateDump> {
    testkit::find_dump(&[dir.to_path_buf()], hit).and_then(|p| StateDump::load(&p).ok())
}

/// `{ n = k, [1] = …, … }` (a function probe's packed results) → the values.
fn unpack_ret(ret: Option<Val>) -> Vec<Val> {
    let Some(Val::Table(t)) = ret else {
        return Vec::new();
    };
    let n = t.get("n").opt_num().unwrap_or(t.len() as f64) as usize;
    (1..=n).map(|i| t.get(i)).collect()
}

/// `a.b[3].c` → `a.b[#].c`.
fn generalize(path: &str) -> String {
    let mut out = String::new();
    let mut chars = path.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '['
            && chars
                .peek()
                .is_some_and(|d| d.is_ascii_digit() || *d == '-' || *d == '@')
        {
            out.push_str("[#]");
            for d in chars.by_ref() {
                if d == ']' {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn clip(s: &str) -> String {
    if s.len() > 160 {
        let end = s.char_indices().nth(160).map_or(s.len(), |(i, _)| i);
        format!("{}…", &s[..end])
    } else {
        s.to_owned()
    }
}

/// One build's L4 result.
struct BuildRun {
    stem: String,
    /// Mismatches of the injected `perform:1943` state against `perform:2308` (non-vacuity).
    before: usize,
    mismatches: Vec<Mismatch>,
    /// Mismatches left out of `mismatches` because replaying `getCachedOutputValue` alone (its
    /// `:in` → `:out`, no PF-D code involved) already produces them ([`replay_alone`]).
    floored: usize,
    /// A failure that is not a state mismatch (inject, panic, unreplayed callee).
    error: Option<String>,
    replayed: Vec<String>,
}

fn run_build(xml: &Path, dir: &Path) -> Result<BuildRun, String> {
    let stem = stem(xml).to_owned();
    let din = load(dir, "perform:1943").ok_or("no perform:1943 dump")?;
    let dout = load(dir, "perform:2308").ok_or("no perform:2308 dump")?;
    let model = testkit::load_build(xml).unwrap_or_else(testkit::empty_build);
    fill_spectre_list(&model, xml);
    let inj = pob_oracle::inject::inject(&din, &model)?;
    let env = inj.env.clone().ok_or("no env in perform:1943")?;
    let locals: Vec<(String, Val)> = inj.locals.into_iter().collect();
    let before = testkit::diff_against(&dout, Some(&env), &locals, &stem).len();

    let mut calls = BTreeMap::new();
    for (site, pin, pout) in SITES {
        if let (Some(a), Some(b)) = (load(dir, pin), load(dir, pout)) {
            calls.insert((*site).to_owned(), Call { din: a, dout: b });
        }
    }
    let replay = Replay { calls };
    let log: Rc<RefCell<(Vec<String>, Vec<String>)>> = Rc::default();
    {
        let env = env.clone();
        let log = log.clone();
        callee_replay::set(Some(Box::new(move |site: &str| {
            register_tooltips(&env);
            match replay.apply_report(site, &env) {
                Ok(applied) => {
                    let mut l = log.borrow_mut();
                    l.0.push(site.to_owned());
                    for n in applied.notes {
                        l.1.push(format!("{site}: {n}"));
                    }
                    Some(unpack_ret(applied.ret))
                }
                Err(e) => {
                    log.borrow_mut()
                        .1
                        .push(format!("{site}: not replayed: {e}"));
                    None
                }
            }
        })));
    }
    pob_calc::stub::stub_reset();
    let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        pob_calc::probe::run_segment(&din.probe, &locals)
    }));
    callee_replay::set(None);
    let stubs = pob_calc::stub::stub_report();
    let (replayed, notes) = log.take();
    let mut error = None;
    let after = match run {
        Ok(Ok((_flow, after))) => after,
        Ok(Err(e)) => return Err(format!("run: {e}")),
        Err(p) => {
            let msg = p
                .downcast_ref::<pob_lua::LuaError>()
                .map(|e| e.0.clone())
                .or_else(|| p.downcast_ref::<String>().cloned())
                .or_else(|| p.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                .unwrap_or_else(|| "panic".to_owned());
            error = Some(format!("panic: {msg}"));
            Vec::new()
        }
    };
    if error.is_none() && !stubs.is_empty() {
        error = Some(format!(
            "unported callees reached (not replayed): {stubs:?}; notes {notes:?}"
        ));
    }
    if error.is_none() && !notes.is_empty() {
        error = Some(format!("replay notes: {notes:?}"));
    }
    if error.is_none() {
        exporter_view(&env);
    }
    let mut mismatches = if error.is_some() {
        Vec::new()
    } else {
        testkit::diff_against(&dout, Some(&env), &after, &stem)
    };
    let mut floored = 0;
    if !mismatches.is_empty() && replayed.iter().any(|s| s == "getCachedOutputValue") {
        // the identity/aliasing differences a getCachedOutputValue replay leaves (its new
        // CALCULATOR env shares gem instances and effects with this env) are the replay's floor
        if let Some(floor) = replay_alone(xml, dir) {
            let key = |m: &Mismatch| format!("{:?} {}", m.kind, m.path);
            let floor: std::collections::BTreeSet<String> = floor.iter().map(key).collect();
            let n = mismatches.len();
            mismatches.retain(|m| !floor.contains(&key(m)));
            floored = n - mismatches.len();
        }
    }
    Ok(BuildRun {
        stem,
        before,
        mismatches,
        floored,
        error,
        replayed,
    })
}

/// `build.spectreList` as `Build:Load` fills it (Build.lua:987-993: every `<Spectre id>` child of
/// `<Build>` whose id is in `data.minions`). The dump does not carry the build model and BM-A's XML
/// loader is not merged yet, so the test reads the list from the XML itself. Skipped when the
/// loader already filled it.
fn fill_spectre_list(model: &pob_calc::BuildRef, xml: &Path) {
    let list = model.spectreList.get();
    if !list.is_empty() {
        return;
    }
    let Ok(text) = std::fs::read_to_string(xml) else {
        return;
    };
    let Some(start) = text.find("<Build ") else {
        return;
    };
    let end = text[start..]
        .find("</Build>")
        .map_or(text.len(), |e| start + e);
    let mut rest = &text[start..end];
    while let Some(i) = rest.find("<Spectre ") {
        rest = &rest[i + 9..];
        let Some(a) = rest.find("id=\"") else { break };
        let after = &rest[a + 4..];
        let Some(b) = after.find('"') else { break };
        let id = &after[..b];
        let name = pob_lua::Name::from(id);
        if pob_gamedata::data().minions.contains_key(&name) {
            list.push(name);
        }
    }
}

/// Two things the ORC-X exporter cannot see by itself (CCR-0093 §3):
///
/// * `env.buffs` / `env.minionBuffs` / `env.debuffs` were nil at `perform:1943`, so injection marks
///   them absent, and the exporter drops an absent typed field that holds a default value (an empty
///   table). The segment assigns them (CalcPerform.lua:2195, 2198, 2200): unmark them.
/// * a requirements row's `sourceNameTooltip` is a `pools_charges::SourceNameTooltip` value (the
///   UI closure of CalcPerform.lua:2139-2141): [`register_tooltips`].
fn exporter_view(env: &pob_calc::env::EnvRef) {
    let p = pob_calc::probe::rc_ptr(env);
    for k in ["buffs", "minionBuffs", "debuffs"] {
        pob_calc::probe::unmark_absent(p, k);
    }
    register_tooltips(env);
}

/// Register every `SourceNameTooltip` in `env.player.breakdown.Req*.rowList` as the `$fn` marker PoB
/// dumps for it. Also done before each replay: a replay writes the whole exported live state back
/// onto the graph, so a value the exporter cannot encode would come back as `$userdata`.
fn register_tooltips(env: &pob_calc::env::EnvRef) {
    use pob_calc::modules::calc_perform::pools_charges::SourceNameTooltip;
    let Some(bd) = env.player.breakdown.get() else {
        return;
    };
    for attr in ["Str", "Dex", "Int", "Omni"] {
        let key = format!("Req{attr}");
        let Some(rows) = bd.t.table(key.as_str()).and_then(|t| t.table("rowList")) else {
            continue;
        };
        for (_, row) in rows.ipairs() {
            let Some(row) = row.table() else { continue };
            if let Val::Obj(o) = row.get("sourceNameTooltip") {
                if (*o).is::<SourceNameTooltip>() {
                    pob_calc::probe::with_registry(|r| {
                        r.set_foreign(
                            o.clone(),
                            serde_json::json!({ "$fn": SourceNameTooltip::SRC }),
                        );
                    });
                }
            }
        }
    }
}

#[derive(Default)]
struct Tally {
    runs: Vec<BuildRun>,
    missing: Vec<String>,
}

impl Tally {
    fn clean(&self) -> usize {
        self.runs
            .iter()
            .filter(|r| r.error.is_none() && r.mismatches.is_empty())
            .count()
    }

    fn report(&self, title: &str) {
        let verbose = std::env::var_os("PFD_VERBOSE").is_some();
        let sites: usize = self.runs.iter().map(|r| r.replayed.len()).sum();
        eprintln!(
            "{title}: {} builds with dumps, {} pass (zero state-diff mismatches), {} fail, {} without dumps; \
             {sites} callee calls replayed",
            self.runs.len(),
            self.clean(),
            self.runs.len() - self.clean(),
            self.missing.len()
        );
        let mut by_path: BTreeMap<String, (usize, String)> = BTreeMap::new();
        for r in &self.runs {
            let status = match (&r.error, r.mismatches.len()) {
                (Some(e), _) => format!("ERROR {}", clip(e)),
                (None, 0) if r.floored > 0 => {
                    format!(
                        "pass ({} mismatches of the getCachedOutputValue replay alone excluded)",
                        r.floored
                    )
                }
                (None, 0) => "pass".to_owned(),
                (None, n) => format!("{n} mismatches"),
            };
            eprintln!(
                "  {:40} before {:5}  after: {status}  (replayed: {})",
                r.stem,
                r.before,
                r.replayed.join(",")
            );
            for m in &r.mismatches {
                let key = format!("{:?} {}", m.kind, generalize(&m.path));
                let e = by_path.entry(key).or_insert_with(|| {
                    (
                        0,
                        format!(
                            "{}: {} pob={} ours={} writer={}",
                            r.stem,
                            m.path,
                            m.pob.as_ref().map_or("-".into(), |x| clip(&x.to_string())),
                            m.ours.as_ref().map_or("-".into(), |x| clip(&x.to_string())),
                            m.writer.as_deref().unwrap_or("-")
                        ),
                    )
                });
                e.0 += 1;
                if verbose {
                    eprintln!(
                        "      {} {:?} pob={:?} ours={:?}",
                        m.path, m.kind, m.pob, m.ours
                    );
                }
            }
        }
        let mut paths: Vec<_> = by_path.into_iter().collect();
        paths.sort_by_key(|p| std::cmp::Reverse(p.1 .0));
        for (p, (n, ex)) in paths.iter().take(40) {
            eprintln!("  {n:6} {p}\n         e.g. {ex}");
        }
        for m in self.missing.iter().take(10) {
            eprintln!("  missing: {m}");
        }
    }
}

fn run_corpus(builds: &[PathBuf]) -> Tally {
    let mut t = Tally::default();
    for xml in builds {
        let Some(dir) = pfd_dir(xml) else {
            t.missing.push(format!("{} (no pfd:* dumps)", stem(xml)));
            continue;
        };
        match run_build(xml, &dir) {
            Ok(r) => t.runs.push(r),
            Err(e) => t.missing.push(format!("{}: {e}", stem(xml))),
        }
    }
    t
}

/// L4 on all57: ≥ 98 % of the builds with dumps pass, every one is non-vacuous.
#[test]
fn pfd_l4_all57() {
    let _serial = serial();
    if !testkit::init_default_data() {
        return skip("no data dump (data/dump)");
    }
    let Some(parity) = find_parity_dir() else {
        return skip("no .parity directory");
    };
    let builds = xmls_in(&parity.join("builds"));
    if builds.is_empty() {
        return skip("no .parity/builds/*.xml");
    }
    let t = run_corpus(&builds);
    t.report("PF-D L4 perform:1943 -> perform:2308 (all57)");
    if t.runs.is_empty() {
        return skip("no PF-D dumps (see the module doc for the oracle.sh command)");
    }
    assert!(
        t.runs.iter().all(|r| r.before > 0),
        "a build whose perform:1943 state already equals perform:2308 (vacuous)"
    );
    let need = (t.runs.len() * 98).div_ceil(100);
    assert!(
        t.clean() >= need,
        "L4 gate: {} of {} builds pass, need {need} (98 %)",
        t.clean(),
        t.runs.len()
    );
}

/// L4 on the unit corpus (`corpus/PF-D/*.xml`): 100 %.
#[test]
fn pfd_l4_unit_corpus() {
    let _serial = serial();
    if !testkit::init_default_data() {
        return skip("no data dump (data/dump)");
    }
    let builds = xmls_in(&unit_corpus());
    if builds.is_empty() {
        return skip("no corpus/PF-D/*.xml");
    }
    let t = run_corpus(&builds);
    t.report("PF-D L4 perform:1943 -> perform:2308 (unit corpus)");
    if t.runs.is_empty() {
        return skip("no PF-D dumps for the unit corpus (see the module doc)");
    }
    assert!(
        t.missing.is_empty(),
        "unit-corpus builds without dumps: {:?}",
        t.missing
    );
    assert_eq!(t.clean(), t.runs.len(), "unit corpus must pass 100 %");
}

/// Replay `getCachedOutputValue` alone on a build: inject its `:in`, apply the recorded delta, diff
/// against its `:out`. No PF-D code runs, so every mismatch is the replay's (ORC-X replay limit:
/// tables the callee creates are copied in, so objects its new env shares with this one by
/// identity come back under other canonical paths). `None` when the build has no such dumps.
fn replay_alone(xml: &Path, dir: &Path) -> Option<Vec<Mismatch>> {
    let din = load(dir, "getCachedOutputValue:in")?;
    let dout = load(dir, "getCachedOutputValue:out")?;
    let model = testkit::load_build(xml).unwrap_or_else(testkit::empty_build);
    fill_spectre_list(&model, xml);
    let inj = pob_oracle::inject::inject(&din, &model).ok()?;
    let env = inj.env.clone()?;
    let mut calls = BTreeMap::new();
    calls.insert(
        din.hit.clone(),
        Call {
            din: din.clone(),
            dout: dout.clone(),
        },
    );
    let applied = Replay { calls }.apply_report(&din.hit, &env).ok()?;
    let mut locals: Vec<(String, Val)> = inj.locals.into_iter().collect();
    if let Some(r) = applied.ret {
        locals.push(("ret".to_owned(), r));
    }
    Some(testkit::diff_against(&dout, Some(&env), &locals, stem(xml)))
}
