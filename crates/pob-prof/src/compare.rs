//! `pob-prof compare BASE NEW`: per-metric deltas with a significance hint, and a non-zero exit
//! on a regression. Owner: PROF1.
//!
//! For every metric present in both files (same scenario, same key) it prints the medians, their
//! relative MADs (the noise of each side), the change of the median in percent, the two-sided
//! Mann-Whitney p-value of the samples, and a verdict:
//!
//! - `REGRESSED`: a gated metric got worse by more than its threshold and p < alpha;
//! - `worse` / `better`: p < alpha, but under the threshold (or not gated);
//! - `~`: not significant (p >= alpha), whatever the delta.
//!
//! Gated by default (all lower-is-better): the steps' `wall_ms`, `cpu_ms`, `frame_p95_ms`, `recalc_ms`,
//! `build_load_ms`, `power_ms`, `allocs`, `heap_peak_mb`, `rss_peak_mb`, and
//! `startup.to_first_screen_ms`, `total.peak_rss_mb` (threshold 10 %, RSS 20 %). `_setup` steps are never gated.
//!
//! NEW is `INCOMPLETE` when a compared scenario has failed repetitions in NEW, when a gated metric
//! of BASE is absent from NEW (a step that no longer runs, a build load that was not detected), or
//! when a scenario named by `--scenarios` is missing from NEW. A scenario missing from NEW without
//! `--scenarios` is listed but not an error (a subset run, `run --scenarios a,b`, compared against
//! the full baseline). The exit status is 1 when any metric REGRESSED or NEW is INCOMPLETE, 0
//! otherwise (2 on usage errors). A summary line gives the geometric mean of the `wall_ms` ratios
//! (new/base) over every gated step: the single number to hill-climb on.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::results::ResultsFile;
use crate::stats::{mann_whitney_p, Summary};

/// Gated metric fields (the part after `<step>.`).
pub const DEFAULT_GATED: &[&str] = &[
    "wall_ms",
    "cpu_ms",
    "frame_p95_ms",
    "recalc_ms",
    "build_load_ms",
    "power_ms",
    "allocs",
    "heap_peak_mb",
    "rss_peak_mb",
    "to_first_screen_ms",
    "peak_rss_mb",
];

#[derive(Clone, Debug)]
pub struct CompareOptions {
    /// Regression threshold in percent of the base median.
    pub threshold_pct: f64,
    /// Per-field thresholds (`allocs=2`), overriding `threshold_pct`.
    pub field_threshold: BTreeMap<String, f64>,
    pub alpha: f64,
    /// Gated fields; empty = [`DEFAULT_GATED`].
    pub gated: Vec<String>,
    /// Print every metric, not only the gated ones and the significant changes.
    pub all: bool,
    /// Only these scenarios (empty = all).
    pub scenarios: Vec<String>,
}

impl Default for CompareOptions {
    fn default() -> Self {
        CompareOptions {
            threshold_pct: 10.0,
            // RSS peaks move by up to ~16 % between runs of the same build (A/A test,
            // docs/perf/PROFILING.md "Noise"); the heap peak is deterministic
            field_threshold: [("rss_peak_mb", 20.0), ("peak_rss_mb", 20.0)]
                .into_iter()
                .map(|(k, v)| (k.to_owned(), v))
                .collect(),
            alpha: 0.05,
            gated: Vec::new(),
            all: false,
            scenarios: Vec::new(),
        }
    }
}

/// One compared metric.
#[derive(Clone, Debug)]
pub struct Row {
    pub scenario: String,
    pub key: String,
    pub base: f64,
    pub base_noise: f64,
    pub new: f64,
    pub new_noise: f64,
    pub delta_pct: f64,
    pub p: f64,
    pub gated: bool,
    pub verdict: &'static str,
}

/// The outcome: the rows, the report text, whether anything regressed.
pub struct Outcome {
    pub rows: Vec<Row>,
    pub report: String,
    pub regressed: bool,
    /// Why NEW cannot be fully compared (module docs, `INCOMPLETE`); empty when it can.
    pub incomplete: Vec<String>,
    pub geomean_wall: Option<f64>,
}

fn field(key: &str) -> &str {
    key.rsplit_once('.').map_or(key, |(_, f)| f)
}

fn step(key: &str) -> &str {
    key.split_once('.').map_or("", |(s, _)| s)
}

fn verdict(
    b: &Summary,
    n: &Summary,
    gated: bool,
    thr: f64,
    alpha: f64,
) -> (&'static str, f64, f64) {
    let delta = if b.median.abs() > 0.0 {
        (n.median - b.median) / b.median.abs() * 100.0
    } else if n.median.abs() > 0.0 {
        100.0
    } else {
        0.0
    };
    let p = mann_whitney_p(&b.samples, &n.samples);
    let v = if p >= alpha {
        "~"
    } else if delta > thr && gated {
        "REGRESSED"
    } else if delta > 0.0 {
        "worse"
    } else if delta < 0.0 {
        "better"
    } else {
        "~"
    };
    (v, delta, p)
}

/// Compare two results files.
pub fn compare(base: &ResultsFile, new: &ResultsFile, o: &CompareOptions) -> Outcome {
    let gated_fields: Vec<String> = if o.gated.is_empty() {
        DEFAULT_GATED.iter().map(|s| (*s).to_owned()).collect()
    } else {
        o.gated.clone()
    };
    let mut rows = Vec::new();
    let mut out = String::new();
    let mb = &base.meta;
    let mn = &new.meta;
    let _ = writeln!(
        out,
        "base: {} {}{} render={} reps={} cpus={} governor={} load={:?}",
        mb.date,
        short(&mb.commit),
        if mb.dirty { "+dirty" } else { "" },
        mb.render,
        mb.reps,
        mb.cpus,
        mb.governor,
        mb.load_start
    );
    let _ = writeln!(
        out,
        "new:  {} {}{} render={} reps={} cpus={} governor={} load={:?}",
        mn.date,
        short(&mn.commit),
        if mn.dirty { "+dirty" } else { "" },
        mn.render,
        mn.reps,
        mn.cpus,
        mn.governor,
        mn.load_start
    );
    for (k, a, b) in [
        ("render mode", &mb.render, &mn.render),
        ("pinned CPUs", &mb.cpus, &mn.cpus),
        ("profile", &mb.profile, &mn.profile),
    ] {
        if a != b {
            let _ = writeln!(
                out,
                "WARNING: {k} differs ({a} vs {b}): the numbers are not comparable"
            );
        }
    }
    let mut ratios = Vec::new();
    let mut cpu_ratios = Vec::new();
    let mut incomplete = Vec::new();
    for name in &o.scenarios {
        if !new.scenarios.contains_key(name) {
            incomplete.push(format!("{name}: missing from NEW"));
        }
    }
    for (name, sb) in &base.scenarios {
        if !o.scenarios.is_empty() && !o.scenarios.contains(name) {
            continue;
        }
        let Some(sn) = new.scenarios.get(name) else {
            let _ = writeln!(out, "{name}: missing from NEW (not compared)");
            continue;
        };
        for e in &sn.errors {
            let _ = writeln!(out, "{name}: NEW failed: {e}");
        }
        if !sn.errors.is_empty() {
            incomplete.push(format!(
                "{name}: {} failed repetition(s) in NEW",
                sn.errors.len()
            ));
        }
        for (key, b) in &sb.metrics {
            let f = field(key);
            let gated = !step(key).starts_with('_') && gated_fields.iter().any(|g| g == f);
            let Some(n) = sn.metrics.get(key) else {
                if gated {
                    incomplete.push(format!("{name}: gated metric {key} is absent from NEW"));
                }
                continue;
            };
            let thr = o.field_threshold.get(f).copied().unwrap_or(o.threshold_pct);
            let (v, delta, p) = verdict(b, n, gated, thr, o.alpha);
            if gated && step(key) != "total" && b.median > 0.0 && n.median > 0.0 {
                if f == "wall_ms" {
                    ratios.push(n.median / b.median);
                } else if f == "cpu_ms" {
                    cpu_ratios.push(n.median / b.median);
                }
            }
            rows.push(Row {
                scenario: name.clone(),
                key: key.clone(),
                base: b.median,
                base_noise: b.rel_mad * 100.0,
                new: n.median,
                new_noise: n.rel_mad * 100.0,
                delta_pct: delta,
                p,
                gated,
                verdict: v,
            });
        }
    }
    for name in new.scenarios.keys() {
        if !base.scenarios.contains_key(name) {
            let _ = writeln!(out, "{name}: new scenario (not in BASE)");
        }
    }
    let _ = writeln!(
        out,
        "\n{:<24} {:<34} {:>11} {:>6} {:>11} {:>6} {:>8} {:>6}  verdict",
        "scenario", "metric", "base", "±%", "new", "±%", "delta%", "p"
    );
    let mut regressed = false;
    for r in &rows {
        if r.verdict == "REGRESSED" {
            regressed = true;
        }
        let show = o.all || r.gated || r.verdict == "better" || r.verdict == "worse";
        if !show {
            continue;
        }
        let _ = writeln!(
            out,
            "{:<24} {:<34} {:>11.3} {:>6.1} {:>11.3} {:>6.1} {:>+8.1} {:>6.3}  {}{}",
            r.scenario,
            r.key,
            r.base,
            r.base_noise,
            r.new,
            r.new_noise,
            r.delta_pct,
            r.p,
            r.verdict,
            if r.gated { "" } else { " (not gated)" }
        );
    }
    let gm = |v: &[f64]| {
        (!v.is_empty()).then(|| (v.iter().map(|r| r.ln()).sum::<f64>() / v.len() as f64).exp())
    };
    let geomean = gm(&ratios);
    let _ = writeln!(out);
    for (what, g, n) in [
        ("wall_ms", geomean, ratios.len()),
        ("cpu_ms", gm(&cpu_ratios), cpu_ratios.len()),
    ] {
        if let Some(g) = g {
            let _ = writeln!(
                out,
                "geomean step {what} ratio (new/base) over {n} steps: {g:.4} ({:+.2}%)",
                (g - 1.0) * 100.0
            );
        }
    }
    let n_reg = rows.iter().filter(|r| r.verdict == "REGRESSED").count();
    let n_better = rows
        .iter()
        .filter(|r| r.verdict == "better" && r.gated)
        .count();
    let _ = writeln!(
        out,
        "{} gated metrics: {n_reg} regressed (> threshold, p < {}), {n_better} better",
        rows.iter().filter(|r| r.gated).count(),
        o.alpha
    );
    if !incomplete.is_empty() {
        let _ = writeln!(
            out,
            "INCOMPLETE: NEW cannot be fully compared ({} problem(s)):",
            incomplete.len()
        );
        for m in &incomplete {
            let _ = writeln!(out, "  {m}");
        }
    }
    Outcome {
        rows,
        report: out,
        regressed,
        incomplete,
        geomean_wall: geomean,
    }
}

fn short(c: &str) -> &str {
    c.get(..10).unwrap_or(c)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::results::{Meta, ScenarioResults};

    fn file(vals: &[(&str, &[f64])]) -> ResultsFile {
        let mut sc = ScenarioResults::default();
        for (k, v) in vals {
            sc.metrics.insert((*k).to_owned(), Summary::of(v));
        }
        let mut f = ResultsFile {
            schema: crate::results::SCHEMA.into(),
            meta: Meta::default(),
            ..ResultsFile::default()
        };
        f.scenarios.insert("s".into(), sc);
        f
    }

    #[test]
    fn regression_gate() {
        let base = file(&[
            ("open.wall_ms", &[100.0, 101.0, 99.0, 100.5, 100.2]),
            ("open.frames", &[5.0; 5]),
            ("_setup.wall_ms", &[10.0, 10.1, 10.2, 9.9, 10.0]),
        ]);
        // +20 % on a gated metric, clearly outside the noise
        let new = file(&[
            ("open.wall_ms", &[120.0, 121.0, 119.0, 120.5, 120.2]),
            ("open.frames", &[5.0; 5]),
            ("_setup.wall_ms", &[20.0, 20.1, 20.2, 19.9, 20.0]),
        ]);
        let o = compare(&base, &new, &CompareOptions::default());
        assert!(o.regressed, "{}", o.report);
        let row = |k: &str| o.rows.iter().find(|r| r.key == k).unwrap().clone();
        assert_eq!(row("open.wall_ms").verdict, "REGRESSED");
        assert_eq!(row("open.frames").verdict, "~");
        // _setup is never gated
        assert_eq!(row("_setup.wall_ms").verdict, "worse");
        assert!((o.geomean_wall.unwrap() - 1.2).abs() < 0.01);
        // a 25 % threshold lets it pass
        let o2 = compare(
            &base,
            &new,
            &CompareOptions {
                threshold_pct: 25.0,
                ..CompareOptions::default()
            },
        );
        assert!(!o2.regressed);
        // the same file never regresses
        assert!(!compare(&base, &base, &CompareOptions::default()).regressed);
    }

    #[test]
    fn incomplete_new_fails() {
        let base = file(&[
            ("open.wall_ms", &[100.0, 101.0, 99.0, 100.5, 100.2]),
            ("open.build_load_ms", &[50.0, 51.0, 49.0, 50.5, 50.2]),
            ("open.frames", &[5.0; 5]),
        ]);
        // the same numbers, but the build load was not detected: a gated metric is absent
        let mut new = file(&[
            ("open.wall_ms", &[100.0, 101.0, 99.0, 100.5, 100.2]),
            ("open.frames", &[5.0; 5]),
        ]);
        let o = compare(&base, &new, &CompareOptions::default());
        assert!(!o.regressed);
        assert_eq!(o.incomplete.len(), 1, "{}", o.report);
        assert!(o.incomplete[0].contains("open.build_load_ms"));
        // an ungated metric absent from NEW is not a problem
        let mut b2 = base.clone();
        b2.scenarios
            .get_mut("s")
            .unwrap()
            .metrics
            .remove("open.build_load_ms");
        assert!(compare(&b2, &new, &CompareOptions::default())
            .incomplete
            .is_empty());
        // failed repetitions in NEW
        new.scenarios
            .get_mut("s")
            .unwrap()
            .errors
            .push("guard failed".into());
        let o = compare(&b2, &new, &CompareOptions::default());
        assert!(
            o.incomplete.iter().any(|m| m.contains("failed repetition")),
            "{}",
            o.report
        );
        // a scenario missing from NEW: listed; an error only when named by --scenarios
        let empty = ResultsFile {
            schema: crate::results::SCHEMA.into(),
            ..ResultsFile::default()
        };
        assert!(compare(&b2, &empty, &CompareOptions::default())
            .incomplete
            .is_empty());
        let named = CompareOptions {
            scenarios: vec!["s".into()],
            ..CompareOptions::default()
        };
        assert_eq!(compare(&b2, &empty, &named).incomplete.len(), 1);
    }

    #[test]
    fn noise_is_not_a_regression() {
        let base = file(&[("open.wall_ms", &[100.0, 130.0, 90.0, 115.0, 95.0])]);
        let new = file(&[("open.wall_ms", &[118.0, 92.0, 128.0, 99.0, 112.0])]);
        let o = compare(&base, &new, &CompareOptions::default());
        assert!(!o.regressed, "{}", o.report);
        assert_eq!(o.rows[0].verdict, "~");
    }
}
