//! T3/T4 parity runner (ui doc §11.2, §11.3): discovers `src/gallery/*` and
//! `scenarios/<unit>/*.pobscript` automatically, so units never edit shared files. Owner: U8.
//!
//! A custom harness (`harness = false`): `cargo test -p pob-ui-oracle --test parity -- [FILTER...]
//! [--list]` (also reached by `cargo test -p pob-ui-oracle -- <unit>`). A case runs when its name
//! contains any FILTER (all cases without one). Cases run one after another (each boots LuaJIT
//! PoB; machine limits, ui doc §11.3).
//!
//! Cases:
//!
//! * `gallery/<name>@<W>x<H>@<S>`: every gallery (`pob_ui_oracle::gallery::GALLERIES`, generated
//!   from `src/gallery/*.rs`) at 1280×800 @1.0 and 1920×1080 @1.5. Passes with zero T3
//!   divergences, no Rust-side error, the same number of frames on both sides, and at least one
//!   draw op (a gallery that draws nothing proves nothing).
//! * `scenario/<unit>/<name>@<W>x<H>@<S>`: every `scenarios/<unit>/*.pobscript` at both screens.
//!   Directories starting with `_` (the oracle's own, `scenarios/_smoke`) are Lua-side acceptance
//!   runs: PoB must run the script without a Lua error and draw. All others are parity runs:
//!   zero divergences, no Rust-side error, identical T4 snapshots. A script may state
//!   expectations in comment lines: `#! expect-text TEXT` (some DrawString of the last frame
//!   contains TEXT) and `#! expect-snapshot NAME` (the Lua side produced snapshot NAME).
//! * `oracle/...`: U8's own acceptance: the diff detects a changed op; two `ui-parity` processes
//!   write byte-identical traces for the smoke gallery, identical draw calls for the build list
//!   at both screens (only the order of `OnInit`'s `ImageLoad` effects, which land in frame 0,
//!   may differ), and for an open build identical traces up to PoB's own string-keyed `pairs`
//!   order (calc doc D8; the sites are written to `order_sites.txt`); audit mode writes its host
//!   list; `ui-parity --gallery smoke` runs both sides end to end.
//!
//! The harness fails when no gallery exists, or when an unfiltered run ran no gallery. A filter
//! that matches no case runs nothing and passes (libtest's rule), so filters aimed at the lib's
//! unit tests do not fail this binary; it says so on stdout.

use std::path::PathBuf;
use std::process::{Command, ExitCode};

use pob_sg::trace::{ApiCall, DiffConfig};
use pob_ui_oracle::diff_report;
use pob_ui_oracle::runner::{self, RunOptions, RunResult, Target};

/// `Send + Sync`: the gallery and scenario cases run on `oracle_cache::par_map` (TP2).
type CaseFn = Box<dyn Fn() -> Result<String, String> + Send + Sync>;

struct Case {
    name: String,
    run: CaseFn,
}

fn screens() -> Vec<(i32, i32, f32)> {
    runner::ACCEPTANCE_SCREENS.to_vec()
}

fn screen_tag((w, h, s): (i32, i32, f32)) -> String {
    format!("{w}x{h}@{s}")
}

fn out_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map_or_else(
            || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target"),
            PathBuf::from,
        )
        .join("ui-parity")
}

fn draw_ops(frames: &[pob_sg::trace::FrameTrace]) -> usize {
    frames
        .iter()
        .flat_map(|f| &f.calls)
        .filter(|c| {
            matches!(
                c.call,
                ApiCall::DrawImage { .. }
                    | ApiCall::DrawImageQuad { .. }
                    | ApiCall::DrawString { .. }
            )
        })
        .count()
}

/// Write the report and require a clean comparison.
fn check_parity(report_name: &str, run: &RunResult) -> Result<String, String> {
    let n = diff_report::write_report(&out_dir(), report_name, run, false)
        .map_err(|e| format!("report: {e:#}"))?;
    let report = out_dir().join(report_name).join("report.txt");
    if let Some(e) = &run.rust_error {
        return Err(format!("Rust side error: {e} (see {})", report.display()));
    }
    if n != 0 {
        return Err(format!("{n} divergences (see {})", report.display()));
    }
    if run.lua.len() != run.rust.len() {
        return Err(format!(
            "frame counts differ: lua {}, rust {}",
            run.lua.len(),
            run.rust.len()
        ));
    }
    let ops = draw_ops(&run.lua);
    if ops == 0 {
        return Err("no draw ops on the Lua side: the run proves nothing".into());
    }
    if run.lua_snapshots != run.rust_snapshots {
        return Err("T4 snapshots differ".into());
    }
    Ok(format!(
        "{} frames, {ops} draw ops, 0 divergences; T3 by {}",
        run.lua.len(),
        if diff_report::uses_u1_diff() {
            "pob_sg::trace (U1)"
        } else {
            "the oracle's local copy (U1-PENDING, CCR-0028 item 13)"
        }
    ))
}

fn gallery_cases() -> Vec<Case> {
    let mut out = Vec::new();
    for name in pob_ui_oracle::gallery::GALLERIES {
        for sc in screens() {
            let name = (*name).to_owned();
            out.push(Case {
                name: format!("gallery/{name}@{}", screen_tag(sc)),
                run: Box::new(move || {
                    let run = runner::run(
                        &Target::Gallery(name.clone()),
                        runner::screen(sc.0, sc.1, sc.2),
                    )
                    .map_err(|e| format!("{e:#}"))?;
                    check_parity(&format!("{name}@{}", screen_tag(sc)), &run)
                }),
            });
        }
    }
    out
}

/// `#! expect-text TEXT` / `#! expect-snapshot NAME` lines of a script.
fn expectations(text: &str) -> (Vec<String>, Vec<String>) {
    let (mut texts, mut snaps) = (Vec::new(), Vec::new());
    for l in text.lines() {
        if let Some(t) = l.trim().strip_prefix("#! expect-text ") {
            texts.push(t.trim().to_owned());
        } else if let Some(t) = l.trim().strip_prefix("#! expect-snapshot ") {
            snaps.push(t.trim().to_owned());
        }
    }
    (texts, snaps)
}

fn check_expectations(run: &RunResult, text: &str) -> Result<(), String> {
    let (texts, snaps) = expectations(text);
    let last = run.lua.last().ok_or("no frames")?;
    for t in texts {
        let found = last.calls.iter().any(|c| match &c.call {
            ApiCall::DrawString { text, .. } => text.contains(&t),
            _ => false,
        });
        if !found {
            return Err(format!("last frame has no DrawString containing {t:?}"));
        }
    }
    for s in snaps {
        match run.lua_snapshots.iter().find(|(n, _)| *n == s) {
            Some((_, xml)) if xml.contains("<PathOfBuilding") => {}
            Some(_) => return Err(format!("snapshot {s} is not a build XML")),
            None => return Err(format!("no snapshot {s}")),
        }
    }
    Ok(())
}

fn scenario_cases() -> Vec<Case> {
    let mut out = Vec::new();
    let root = runner::crate_dir().join("scenarios");
    let mut units: Vec<PathBuf> = std::fs::read_dir(&root)
        .map(|rd| {
            rd.filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect()
        })
        .unwrap_or_default();
    units.sort();
    for unit_dir in units {
        let unit = unit_dir.file_name().unwrap().to_string_lossy().into_owned();
        let mut scripts: Vec<PathBuf> = std::fs::read_dir(&unit_dir)
            .map(|rd| {
                rd.filter_map(Result::ok)
                    .map(|e| e.path())
                    .filter(|p| p.extension().is_some_and(|x| x == "pobscript"))
                    .collect()
            })
            .unwrap_or_default();
        scripts.sort();
        for script in scripts {
            let stem = script.file_stem().unwrap().to_string_lossy().into_owned();
            for sc in screens() {
                let (unit, stem, script) = (unit.clone(), stem.clone(), script.clone());
                let lua_only = unit.starts_with('_');
                out.push(Case {
                    name: format!("scenario/{unit}/{stem}@{}", screen_tag(sc)),
                    run: Box::new(move || {
                        let text = std::fs::read_to_string(&script).map_err(|e| e.to_string())?;
                        let (parsed, base_dir) =
                            runner::load_script(&script).map_err(|e| format!("{e:#}"))?;
                        let run = runner::run_with(
                            &Target::Scenario {
                                script: parsed,
                                base_dir,
                            },
                            &RunOptions {
                                screen: runner::screen(sc.0, sc.1, sc.2),
                                audit: false,
                                lua_only,
                            },
                        )
                        .map_err(|e| format!("{e:#}"))?;
                        check_expectations(&run, &text)?;
                        let report = format!("{unit}_{stem}@{}", screen_tag(sc));
                        if lua_only {
                            diff_report::write_report(&out_dir(), &report, &run, false)
                                .map_err(|e| format!("report: {e:#}"))?;
                            let ops = draw_ops(&run.lua);
                            if ops == 0 {
                                return Err("no draw ops".into());
                            }
                            Ok(format!(
                                "Lua side: {} frames, {ops} draw ops",
                                run.lua.len()
                            ))
                        } else {
                            check_parity(&report, &run)
                        }
                    }),
                });
            }
        }
    }
    out
}

fn ui_parity(args: &[&str]) -> Result<std::process::Output, String> {
    Command::new(env!("CARGO_BIN_EXE_ui-parity"))
        .args(args)
        .output()
        .map_err(|e| format!("ui-parity: {e}"))
}

/// Run `ui-parity` twice (Lua side only) and return both `lua.jsonl` files.
fn two_runs(first: &str, rest: &[&str], name: &str) -> Result<Vec<Vec<u8>>, String> {
    let mut traces = Vec::new();
    for i in 1..=2 {
        let n = format!("{name}_run{i}");
        let mut args = vec![first];
        args.extend_from_slice(rest);
        args.extend_from_slice(&["--lua-only", "--name", &n]);
        let o = ui_parity(&args)?;
        if !o.status.success() {
            return Err(format!(
                "run {i} failed: {}",
                String::from_utf8_lossy(&o.stderr)
            ));
        }
        let p = out_dir().join(&n).join("lua.jsonl");
        let t = std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        if t.is_empty() {
            return Err(format!("{}: empty trace", p.display()));
        }
        traces.push(t);
    }
    Ok(traces)
}

fn parse_jsonl(b: &[u8]) -> Result<Vec<pob_sg::trace::FrameTrace>, String> {
    String::from_utf8_lossy(b)
        .lines()
        .map(|l| serde_json::from_str(l).map_err(|e| e.to_string()))
        .collect()
}

fn smoke_script(name: &str) -> String {
    runner::crate_dir()
        .join("scenarios/_smoke")
        .join(format!("{name}.pobscript"))
        .to_string_lossy()
        .into_owned()
}

fn oracle_cases() -> Vec<Case> {
    let mut out: Vec<Case> = Vec::new();
    // The comparison is not tautological: a changed op on the Rust side is reported.
    out.push(Case {
        name: "oracle/diff_detects_change".into(),
        run: Box::new(|| {
            let mut run = runner::run(
                &Target::Gallery("smoke".into()),
                runner::screen(1280, 800, 1.0),
            )
            .map_err(|e| format!("{e:#}"))?;
            let cfg = DiffConfig::default();
            if !diff_report::diff_run(&run, &cfg).is_empty() {
                return Err("smoke gallery diverges before the change".into());
            }
            let f = run.rust.last_mut().ok_or("no Rust frames")?;
            let c = f
                .calls
                .iter_mut()
                .find(|c| matches!(c.call, ApiCall::DrawString { .. }))
                .ok_or("no DrawString")?;
            if let ApiCall::DrawString { x, .. } = &mut c.call {
                *x += 1e-3;
            }
            let n = diff_report::diff_run(&run, &cfg).len();
            if n == 0 {
                return Err("a moved DrawString was not detected".into());
            }
            run.rust.pop();
            let d = diff_report::diff_run(&run, &cfg);
            if d.is_empty() {
                return Err("a missing frame was not detected".into());
            }
            Ok(format!(
                "moved op: {n} divergent frame(s); missing frame detected"
            ))
        }),
    });
    // Two runs (two processes) give identical draw calls for the build list (both screens). The
    // `ImageLoad` effects of `OnInit` (PassiveTree loads its sprites while iterating
    // string-keyed tables, calc doc D8) land in frame 0 and may come in another order; nothing
    // else may differ.
    for (script, dpi) in [("boot_list", "1"), ("boot_list", "1.5")] {
        out.push(Case {
            name: format!("oracle/determinism/{script}@{dpi}"),
            run: Box::new(move || {
                let traces = two_runs(
                    &smoke_script(script),
                    &["--dpi", dpi],
                    &format!("det_{script}_{dpi}"),
                )?;
                if traces[0] == traces[1] {
                    return Ok(format!("{} bytes identical", traces[0].len()));
                }
                let sites = diff_report::order_only_difference(
                    &parse_jsonl(&traces[0])?,
                    &parse_jsonl(&traces[1])?,
                )?;
                if let Some(s) = sites
                    .iter()
                    .find(|s| !s.starts_with("side effects of frame"))
                {
                    return Err(format!("the two runs' draw calls differ in order at {s}"));
                }
                Ok(format!(
                    "{} bytes, identical draw calls; effect order differs in: {}",
                    traces[0].len(),
                    sites.join(", ")
                ))
            }),
        });
    }
    out.push(Case {
        name: "oracle/determinism/gallery_smoke".into(),
        run: Box::new(|| {
            let traces = two_runs("--gallery", &["smoke"], "det_gallery_smoke")?;
            if traces[0] != traces[1] {
                return Err("the two runs' lua.jsonl differ".into());
            }
            Ok(format!("{} bytes identical", traces[0].len()))
        }),
    });
    // An open build draws the passive tree, which iterates string-keyed tables with `pairs`
    // (PoB itself is nondeterministic there, calc doc D8): the two runs must agree up to call
    // order, and the sites whose order differed are listed (work for the units that port them,
    // ui doc §8 rule 6).
    out.push(Case {
        name: "oracle/determinism/boot_build".into(),
        run: Box::new(|| {
            let traces = two_runs(&smoke_script("boot_build"), &[], "det_boot_build")?;
            if traces[0] == traces[1] {
                return Ok(format!("{} bytes identical", traces[0].len()));
            }
            let sites = diff_report::order_only_difference(
                &parse_jsonl(&traces[0])?,
                &parse_jsonl(&traces[1])?,
            )?;
            let file = out_dir()
                .join("det_boot_build_run1")
                .join("order_sites.txt");
            std::fs::write(&file, sites.join("\n") + "\n").map_err(|e| e.to_string())?;
            Ok(format!(
                "identical up to PoB's pairs order at {} sites ({})",
                sites.len(),
                file.display()
            ))
        }),
    });
    // A Lua error that PoB's own PCall catches (it shows launch:ShowErrMsg's prompt instead of
    // escaping) fails the run at the next frame.
    out.push(Case {
        name: "oracle/error_prompt_fails_run".into(),
        run: Box::new(|| {
            let pob_root = pob_ui_oracle::boot::locate_pob_root().ok_or("PoB not found")?;
            let user = runner::UserDir::new(Some(&runner::crate_dir().join("fixtures/user1")))
                .map_err(|e| format!("{e:#}"))?;
            let handles = pob_ui_oracle::boot::OracleHandles::default();
            let mut app = runner::lua_side(
                &pob_root,
                &user,
                &handles,
                pob_ui_oracle::boot::Entry::Launch,
                false,
            )
            .map_err(|e| format!("{e:#}"))?;
            let mut d =
                runner::Driver::new(runner::screen(1280, 800, 1.0), handles, user.path.clone());
            let script = runner::parse_script("frames 1")?;
            let (_, r) = runner::drive(&mut app, &script, &mut d);
            r.map_err(|e| format!("clean boot failed: {e:#}"))?;
            app.with_vm(|pob| pob.eval::<()>("launch:ShowErrMsg('boom %d', 1)"))
                .map_err(|e| e.to_string())?;
            match d.frame(&mut app) {
                Err(e) if e.to_string().contains("boom 1") => Ok(format!("run failed: {e}")),
                Err(e) => Err(format!("failed for another reason: {e:#}")),
                Ok(()) => Err("the error prompt did not fail the run".into()),
            }
        }),
    });
    // Audit mode runs (twice, in child processes) and writes its host list.
    out.push(Case {
        name: "oracle/audit".into(),
        run: Box::new(|| {
            let o = ui_parity(&[
                &smoke_script("boot_build"),
                "--audit",
                "--name",
                "audit_boot_build",
            ])?;
            if !o.status.success() {
                return Err(format!(
                    "audit failed: {}",
                    String::from_utf8_lossy(&o.stderr)
                ));
            }
            let dir = out_dir().join("audit_boot_build");
            let hits =
                diff_report::read_audit(&dir.join("audit.txt")).map_err(|e| format!("{e:#}"))?;
            for run in 1..=2 {
                diff_report::read_audit(&dir.join(format!("audit_run{run}.txt")))
                    .map_err(|e| format!("{e:#}"))?;
            }
            Ok(format!("audit.txt: {} hosts", hits.len()))
        }),
    });
    // `ui-parity --gallery smoke` runs both sides end to end.
    out.push(Case {
        name: "oracle/cli_gallery_smoke".into(),
        run: Box::new(|| {
            let o = ui_parity(&["--gallery", "smoke", "--name", "cli_gallery_smoke"])?;
            let stdout = String::from_utf8_lossy(&o.stdout).into_owned();
            if !o.status.success() {
                return Err(format!(
                    "exit {:?}: {stdout}{}",
                    o.status.code(),
                    String::from_utf8_lossy(&o.stderr)
                ));
            }
            let dir = out_dir().join("cli_gallery_smoke");
            for f in ["lua.jsonl", "rust.jsonl", "report.txt"] {
                let p = dir.join(f);
                if std::fs::metadata(&p).map_or(true, |m| m.len() == 0) {
                    return Err(format!("{} missing or empty", p.display()));
                }
            }
            Ok(stdout.trim().to_owned())
        }),
    });
    out
}

fn main() -> ExitCode {
    let mut filters: Vec<String> = Vec::new();
    let mut list = false;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--list" => list = true,
            // libtest options that take a value.
            "--test-threads" | "--format" | "--color" | "--skip" | "-Z" | "--logfile" => {
                let _ = args.next();
            }
            s if s.starts_with('-') => {}
            s => filters.push(s.to_owned()),
        }
    }
    if pob_ui_oracle::gallery::GALLERIES.is_empty() {
        eprintln!("parity: no gallery was generated from src/gallery/*.rs (build.rs)");
        return ExitCode::FAILURE;
    }
    let mut cases = gallery_cases();
    cases.extend(scenario_cases());
    cases.extend(oracle_cases());
    let selected: Vec<&Case> = cases
        .iter()
        .filter(|c| filters.is_empty() || filters.iter().any(|f| c.name.contains(f.as_str())))
        .collect();
    if list {
        for c in &selected {
            println!("{}: test", c.name);
        }
        return ExitCode::SUCCESS;
    }
    if pob_ui_oracle::boot::locate_pob_root().is_none() {
        eprintln!("parity: PoB not found (set POB_ROOT to a .PathOfBuilding dir)");
        return ExitCode::FAILURE;
    }
    if selected.is_empty() {
        // libtest's rule: a filter that matches nothing runs 0 tests and passes, so a filter
        // meant for the lib's unit tests (`cargo test -p pob-ui-oracle -- canon_pairs`) does not
        // fail this binary. The case list is printed so a mistyped unit name is visible.
        println!(
            "running 0 parity cases: no case matches {filters:?} ({} cases exist; `--list` shows them)",
            cases.len()
        );
        return ExitCode::SUCCESS;
    }
    println!("running {} parity cases", selected.len());
    let (mut passed, mut failed) = (0usize, Vec::new());
    let mut galleries_ran = 0usize;
    // TP2: the gallery and scenario cases are independent runs (each its own Lua VM, user dir,
    // Rust app and report directory): they run on the bounded pool (`POB_TEST_PAR`, fresh thread
    // per case), the `oracle/` cases (determinism runs of the CLI, audit, ...) after them, one at
    // a time as before. Results are printed in case order.
    let run_case = |c: &Case| {
        let t0 = std::time::Instant::now();
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (c.run)()))
            .unwrap_or_else(|p| Err(format!("panicked: {}", pob_host::panic_message(&*p))));
        (r, t0.elapsed().as_secs_f64())
    };
    let (pooled, serial): (Vec<&Case>, Vec<&Case>) = selected
        .into_iter()
        .partition(|c| c.name.starts_with("gallery/") || c.name.starts_with("scenario/"));
    let pooled_results = pob_ui_oracle::oracle_cache::par_map(pooled.clone(), run_case);
    let results = pooled
        .into_iter()
        .zip(pooled_results)
        .chain(serial.into_iter().map(|c| (c, run_case(c))));
    for (c, (r, dt)) in results {
        match r {
            Ok(msg) => {
                println!("test {} ... ok ({dt:.1}s; {msg})", c.name);
                passed += 1;
                if c.name.starts_with("gallery/") {
                    galleries_ran += 1;
                }
            }
            Err(e) => {
                println!("test {} ... FAILED ({dt:.1}s)\n    {e}", c.name);
                failed.push(c.name.clone());
            }
        }
    }
    println!(
        "\nparity: {passed} passed; {} failed; {galleries_ran} gallery runs passed",
        failed.len()
    );
    if !failed.is_empty() {
        for f in &failed {
            println!("    {f}");
        }
        return ExitCode::FAILURE;
    }
    if passed == 0 || (filters.is_empty() && galleries_ran == 0) {
        println!("parity: no gallery ran");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
