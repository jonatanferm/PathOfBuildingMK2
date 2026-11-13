//! Profiling scenarios: `.pobscript` (pob-host's scenario DSL, `pob_host::script`) plus a few
//! directives of the suite's own. Owner: PROF1.
//!
//! A scenario is a `.pobscript` file in `crates/pob-prof/scenarios/`. Every line the suite does not
//! recognise below is parsed by `pob_host::script::Script::parse` (`screen`, `fixture`,
//! `load-build`, `frames`, `move`, `click`, `key-down`, `key-up`, `press`, `type`, `wheel`,
//! `paste-clipboard`, `snapshot`, `exit`; see that module's table). The suite's directives each
//! stand on a line of their own:
//!
//! | directive | meaning |
//! |---|---|
//! | `step NAME` | starts the measured step `NAME` (ends the previous one). Lines before the first `step` form the step `_setup` (measured, never gated) |
//! | `expect-recalc [N]` | guard: the current step has run at least N full recalcs so far (default 1) |
//! | `expect-no-recalc` | guard: the current step has run no full recalc so far |
//! | `expect-misc [N]` | guard: the current step has called the misc calculator at least N times (tooltips with stat differences; default 1) |
//! | `expect-file-write [N]` | guard: the current step has written at least N files (a save; default 1) |
//! | `expect-mode NAME` | guard: `main.mode` is NAME (`LIST`, `BUILD`) |
//! | `expect-tab NAME` | guard: the open build's `viewMode` is NAME (`TREE`, `SKILLS`, `ITEMS`, `CALCS`, `CONFIG`, `IMPORT`, `NOTES`, `PARTY`, `COMPARE`) |
//! | `expect-power` | guard: a power report has finished in this step (`wait-power` ran to its end) |
//! | `expect-differs A B` / `expect-same A B` | guard: the build XML of snapshots A and B (`snapshot NAME`) differ / are equal |
//! | `expect-calc-func` | guard: the build has a misc calculator (`configTab.calcFunc`) |
//! | `expect-text "S"` / `expect-no-text "S"` | guard: one extra frame is drawn with a `TraceRecorder`, and some / none of its drawn strings (escapes stripped) contains S. For clicks and keys whose effect shows only on screen (a sub-view, a pinned breakdown, a typed search). The extra frame is not one of the step's frames and its time and allocations leave the step, but the app does run it |
//! | `wait-idle [MAX]` | runs frames until no background coroutine is alive (`App::wants_frames`: the item database loader of `main:Init`, list loaders, …; at most MAX frames, default 1000; fails at MAX) |
//! | `wait-power [MAX]` | runs frames until no power report is running (at most MAX frames, default 3000; the guard fails at MAX) |
//! | `clipboard-build-code "Builds/x.xml"` | puts the build code (`ImportTab`'s export encoding) of a fixture build on the virtual clipboard; untimed |
//!
//! Guards and snapshots are untimed: their time and allocations are subtracted from the step.
//! The first comment line is the scenario's description.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use pob_host::script::{Script, Step};

/// One element of a scenario, in file order.
#[derive(Clone, Debug, PartialEq)]
pub enum Seg {
    /// `step NAME`
    Step(String),
    /// pob-host steps (one or more consecutive lines)
    Script(Vec<Step>),
    Guard(Guard),
    /// `wait-power [MAX]`
    WaitPower(u32),
    /// `wait-idle [MAX]`
    WaitIdle(u32),
    /// `clipboard-build-code "path"`
    ClipboardBuildCode(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Guard {
    Recalc(u64),
    NoRecalc,
    Misc(u64),
    FileWrite(u64),
    Mode(String),
    Tab(String),
    Power,
    Differs(String, String),
    Same(String, String),
    CalcFunc,
    /// `expect-text "S"`
    Text(String),
    /// `expect-no-text "S"`
    NoText(String),
}

/// A parsed scenario.
#[derive(Clone, Debug)]
pub struct Scenario {
    pub name: String,
    pub description: String,
    pub path: PathBuf,
    pub segs: Vec<Seg>,
}

/// Split a directive line into words (`"…"` quoting with `\"` and `\\`; `#` ends the line).
fn words(line: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut cur: Option<String> = None;
    let mut it = line.chars();
    while let Some(c) = it.next() {
        match c {
            '#' => break,
            c if c.is_whitespace() => {
                if let Some(w) = cur.take() {
                    out.push(w);
                }
            }
            '"' => {
                let w = cur.get_or_insert_with(String::new);
                loop {
                    match it.next() {
                        None => return Err("unterminated string".into()),
                        Some('"') => break,
                        Some('\\') => match it.next() {
                            Some(e @ ('"' | '\\')) => w.push(e),
                            _ => return Err("bad escape".into()),
                        },
                        Some(c) => w.push(c),
                    }
                }
            }
            c => cur.get_or_insert_with(String::new).push(c),
        }
    }
    if let Some(w) = cur.take() {
        out.push(w);
    }
    Ok(out)
}

const DIRECTIVES: &[&str] = &[
    "step",
    "expect-recalc",
    "expect-no-recalc",
    "expect-misc",
    "expect-file-write",
    "expect-mode",
    "expect-tab",
    "expect-power",
    "expect-differs",
    "expect-same",
    "expect-calc-func",
    "expect-text",
    "expect-no-text",
    "wait-power",
    "wait-idle",
    "clipboard-build-code",
];

fn directive(w: &[String]) -> Result<Seg, String> {
    let argc = |n: usize| -> Result<(), String> {
        if w.len() == n + 1 {
            Ok(())
        } else {
            Err(format!("`{}` takes {n} argument(s)", w[0]))
        }
    };
    let num = |i: usize, d: u64| -> Result<u64, String> {
        match w.get(i) {
            None => Ok(d),
            Some(s) => s
                .parse::<u64>()
                .map_err(|_| format!("`{}`: {s:?} is not a count", w[0])),
        }
    };
    Ok(match w[0].as_str() {
        "step" => {
            argc(1)?;
            if w[1].starts_with('_') {
                return Err("step names starting with `_` are reserved".into());
            }
            Seg::Step(w[1].clone())
        }
        "expect-recalc" => {
            if w.len() > 2 {
                return Err("usage: expect-recalc [N]".into());
            }
            Seg::Guard(Guard::Recalc(num(1, 1)?))
        }
        "expect-misc" => {
            if w.len() > 2 {
                return Err("usage: expect-misc [N]".into());
            }
            Seg::Guard(Guard::Misc(num(1, 1)?))
        }
        "expect-file-write" => {
            if w.len() > 2 {
                return Err("usage: expect-file-write [N]".into());
            }
            Seg::Guard(Guard::FileWrite(num(1, 1)?))
        }
        "expect-no-recalc" => {
            argc(0)?;
            Seg::Guard(Guard::NoRecalc)
        }
        "expect-mode" => {
            argc(1)?;
            Seg::Guard(Guard::Mode(w[1].clone()))
        }
        "expect-tab" => {
            argc(1)?;
            Seg::Guard(Guard::Tab(w[1].clone()))
        }
        "expect-power" => {
            argc(0)?;
            Seg::Guard(Guard::Power)
        }
        "expect-differs" => {
            argc(2)?;
            Seg::Guard(Guard::Differs(w[1].clone(), w[2].clone()))
        }
        "expect-same" => {
            argc(2)?;
            Seg::Guard(Guard::Same(w[1].clone(), w[2].clone()))
        }
        "expect-calc-func" => {
            argc(0)?;
            Seg::Guard(Guard::CalcFunc)
        }
        "expect-text" | "expect-no-text" => {
            argc(1)?;
            if w[1].is_empty() {
                return Err(format!("`{}`: the text is empty", w[0]));
            }
            Seg::Guard(if w[0] == "expect-text" {
                Guard::Text(w[1].clone())
            } else {
                Guard::NoText(w[1].clone())
            })
        }
        "wait-power" => {
            if w.len() > 2 {
                return Err("usage: wait-power [MAX]".into());
            }
            Seg::WaitPower(u32::try_from(num(1, 3000)?).map_err(|e| e.to_string())?)
        }
        "wait-idle" => {
            if w.len() > 2 {
                return Err("usage: wait-idle [MAX]".into());
            }
            Seg::WaitIdle(u32::try_from(num(1, 1000)?).map_err(|e| e.to_string())?)
        }
        "clipboard-build-code" => {
            argc(1)?;
            Seg::ClipboardBuildCode(w[1].clone())
        }
        _ => unreachable!("not a directive"),
    })
}

impl Scenario {
    /// Parse scenario text (`path` names it in errors and is the base of relative fixture paths).
    pub fn parse(text: &str, path: &Path) -> Result<Scenario> {
        let mut segs: Vec<Seg> = Vec::new();
        let mut description = String::new();
        for (i, line) in text.lines().enumerate() {
            let at = || format!("{}:{}", path.display(), i + 1);
            let t = line.trim();
            if description.is_empty() {
                if let Some(c) = t.strip_prefix('#') {
                    description = c.trim().to_owned();
                    continue;
                }
            }
            let w = words(line).map_err(|e| anyhow!("{}: {e}", at()))?;
            if w.is_empty() {
                continue;
            }
            if DIRECTIVES.contains(&w[0].as_str()) {
                segs.push(directive(&w).map_err(|e| anyhow!("{}: {e}", at()))?);
                continue;
            }
            let s = Script::parse(line).map_err(|e| anyhow!("{}: {e}", at()))?;
            if let Some(Seg::Script(v)) = segs.last_mut() {
                v.extend(s.steps);
            } else {
                segs.push(Seg::Script(s.steps));
            }
        }
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut seen = std::collections::BTreeSet::new();
        for s in &segs {
            if let Seg::Step(n) = s {
                if !seen.insert(n.clone()) {
                    return Err(anyhow!("{}: step {n:?} appears twice", path.display()));
                }
            }
        }
        Ok(Scenario {
            name,
            description,
            path: path.to_path_buf(),
            segs,
        })
    }

    /// Read and parse a scenario file.
    pub fn load(path: &Path) -> Result<Scenario> {
        let text = std::fs::read_to_string(path).map_err(|e| anyhow!("{}: {e}", path.display()))?;
        Scenario::parse(&text, path)
    }

    /// The directory relative fixture paths resolve against.
    pub fn base_dir(&self) -> PathBuf {
        self.path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default()
    }

    /// The step names, in order (`_setup` first when anything precedes the first `step`).
    pub fn step_names(&self) -> Vec<String> {
        let mut out = Vec::new();
        // `_setup` exists when anything but the leading `screen`/`fixture` lines precedes the
        // first `step`
        let lead_only = |s: &Seg| match s {
            Seg::Script(v) => v
                .iter()
                .all(|x| matches!(x, Step::Screen { .. } | Step::Fixture { .. })),
            _ => false,
        };
        if self
            .segs
            .iter()
            .take_while(|s| !matches!(s, Seg::Step(_)))
            .any(|s| !lead_only(s))
        {
            out.push("_setup".to_owned());
        }
        for s in &self.segs {
            if let Seg::Step(n) = s {
                out.push(n.clone());
            }
        }
        out
    }
}

/// Every `*.pobscript` in `dir`, sorted by name.
pub fn list(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| anyhow!("{}: {e}", dir.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "pobscript"))
        .collect();
    v.sort();
    Ok(v)
}

/// A scenario by name (`tree_alloc`) or path.
pub fn find(dir: &Path, name: &str) -> Result<PathBuf> {
    let p = Path::new(name);
    if p.extension().is_some_and(|x| x == "pobscript") && p.is_file() {
        return Ok(p.to_path_buf());
    }
    let q = dir.join(format!("{name}.pobscript"));
    if q.is_file() {
        Ok(q)
    } else {
        Err(anyhow!("no scenario {name:?} in {}", dir.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_directives_and_script() {
        let s = Scenario::parse(
            "# A test scenario\nscreen 1920 1080\nfixture userdir=../fixtures/user\nframes 2\nstep open\nload-build \"Builds/a b.xml\"\nframes 3\nexpect-recalc 2\nexpect-mode BUILD\nsnapshot one\nstep hover\nmove 1 2 ; frames 1\nwait-power 50\nexpect-power\nclipboard-build-code \"Builds/x.xml\" # c\nexpect-differs one two\n",
            Path::new("/x/t.pobscript"),
        )
        .unwrap();
        assert_eq!(s.name, "t");
        assert_eq!(s.description, "A test scenario");
        assert_eq!(s.step_names(), vec!["_setup", "open", "hover"]);
        assert_eq!(s.segs.len(), 12);
        assert_eq!(s.segs[1], Seg::Step("open".into()));
        assert_eq!(
            s.segs[2],
            Seg::Script(vec![
                Step::LoadBuild("Builds/a b.xml".into()),
                Step::Frames(3)
            ])
        );
        assert_eq!(s.segs[3], Seg::Guard(Guard::Recalc(2)));
        assert_eq!(s.segs[5], Seg::Script(vec![Step::Snapshot("one".into())]));
        assert_eq!(
            s.segs[7],
            Seg::Script(vec![Step::Move(1.0, 2.0), Step::Frames(1)])
        );
        assert_eq!(s.segs[8], Seg::WaitPower(50));
        assert_eq!(s.segs[10], Seg::ClipboardBuildCode("Builds/x.xml".into()));
        assert_eq!(
            s.segs[11],
            Seg::Guard(Guard::Differs("one".into(), "two".into()))
        );
    }

    #[test]
    fn errors_name_the_line() {
        for (text, needle) in [
            ("# d\nframes x", "t.pobscript:2"),
            ("# d\nstep a\nstep a", "appears twice"),
            ("# d\nstep _x", "reserved"),
            ("# d\nexpect-recalc x", "not a count"),
            ("# d\nexpect-mode", "takes 1"),
            ("# d\nclipboard-build-code \"x", "unterminated"),
            ("# d\nexpect-text \"\"", "empty"),
            ("# d\nexpect-no-text a b", "takes 1"),
        ] {
            let e = Scenario::parse(text, Path::new("t.pobscript"))
                .unwrap_err()
                .to_string();
            assert!(e.contains(needle), "{text:?}: {e}");
        }
    }

    #[test]
    fn parses_text_guards() {
        let s = Scenario::parse(
            "# d\nstep a\nexpect-text \"Calc Mode:\"\nexpect-no-text \"1,879 (base)\"\n",
            Path::new("t.pobscript"),
        )
        .unwrap();
        assert_eq!(s.segs[1], Seg::Guard(Guard::Text("Calc Mode:".into())));
        assert_eq!(s.segs[2], Seg::Guard(Guard::NoText("1,879 (base)".into())));
    }

    /// Every committed scenario parses and names its steps.
    #[test]
    fn committed_scenarios_parse() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("scenarios");
        let all = list(&dir).unwrap();
        assert!(all.len() >= 12, "{} scenarios", all.len());
        for p in all {
            let s = Scenario::load(&p).unwrap();
            assert!(!s.description.is_empty(), "{}: no description", p.display());
            assert!(s.step_names().len() >= 2, "{}: no step", p.display());
        }
    }
}
