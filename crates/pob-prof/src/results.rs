//! The results file (`pob-prof/1`): every repetition's raw measurements plus a per-metric summary
//! over the measured repetitions (warm-up ones dropped). Owner: PROF1.
//!
//! ```json
//! { "schema": "pob-prof/1",
//!   "meta": { "commit": "…", "dirty": false, "date": "…", "render": "none", "reps": 5, "warmup": 1,
//!             "cpus": "2-7", "governor": "schedutil", "cpu_model": "…", "kernel": "…",
//!             "load_start": [..3], "load_end": [..3], "profile": "prof", "label": "…" },
//!   "scenarios": { "<name>": { "description": "…", "steps": ["_setup", "open", …],
//!       "metrics": { "<step>.<metric>": { "median", "mad", "rel_mad", "min", "max", "n", "samples" } },
//!       "reps": [ <RepResult>, … ] } } }
//! ```
//!
//! Metric keys are `<step>.<field>` for every numeric [`StepResult`] field, `startup.<field>`,
//! `total.wall_ms`, `total.cpu_ms` and `total.peak_rss_mb`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::driver::{RepResult, StepResult};
use crate::stats::Summary;

pub const SCHEMA: &str = "pob-prof/1";

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Meta {
    pub commit: String,
    pub dirty: bool,
    pub date: String,
    pub label: String,
    pub render: String,
    pub reps: u32,
    pub warmup: u32,
    pub profile: String,
    pub cpus: String,
    pub governor: String,
    pub cpu_model: String,
    pub kernel: String,
    pub load_start: Vec<f64>,
    pub load_end: Vec<f64>,
    pub wall_s: f64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ScenarioResults {
    pub description: String,
    pub steps: Vec<String>,
    pub metrics: BTreeMap<String, Summary>,
    /// Every repetition, warm-up ones included (`rep < meta.warmup`).
    pub reps: Vec<RepResult>,
    /// Repetitions that failed (their error).
    pub errors: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ResultsFile {
    pub schema: String,
    pub meta: Meta,
    pub scenarios: BTreeMap<String, ScenarioResults>,
}

/// The numeric fields of a step, by name (`None` fields are skipped).
pub fn step_metrics(s: &StepResult) -> Vec<(&'static str, f64)> {
    let mut v = vec![
        ("wall_ms", s.wall_ms),
        ("cpu_ms", s.cpu_ms),
        ("frames", s.frames as f64),
        ("frame_p50_ms", s.frame_p50_ms),
        ("frame_p95_ms", s.frame_p95_ms),
        ("frame_max_ms", s.frame_max_ms),
        ("frame_sum_ms", s.frame_sum_ms),
        ("input_ms", s.input_ms),
        ("render_ms", s.render_ms),
        ("tab_draw_ms", s.tab_draw_ms),
        ("recalcs", s.recalcs as f64),
        ("recalc_ms", s.recalc_ms),
        ("recalc_max_ms", s.recalc_max_ms),
        ("misc_gets", s.misc_gets as f64),
        ("misc_calls", s.misc_calls as f64),
        ("misc_ms", s.misc_ms),
        ("allocs", s.allocs as f64),
        ("alloc_mb", s.alloc_mb),
        ("heap_peak_mb", s.heap_peak_mb),
        ("rss_peak_mb", s.rss_peak_mb),
    ];
    if let Some(x) = s.build_load_ms {
        v.push(("build_load_ms", x));
    }
    if let Some(x) = s.power_ms {
        v.push(("power_ms", x));
    }
    if s.frames == 0 {
        // a step without frames (the calc stages): no frame statistics
        v.retain(|(k, _)| {
            !matches!(
                *k,
                "frames"
                    | "frame_p50_ms"
                    | "frame_p95_ms"
                    | "frame_max_ms"
                    | "frame_sum_ms"
                    | "render_ms"
                    | "tab_draw_ms"
                    | "input_ms"
                    | "rss_peak_mb"
                    | "heap_peak_mb"
                    | "recalcs"
                    | "recalc_ms"
                    | "recalc_max_ms"
                    | "misc_gets"
                    | "misc_calls"
                    | "misc_ms"
                    | "alloc_mb"
            )
        });
    }
    v.retain(|(_, x)| x.is_finite());
    v
}

/// Every metric of one repetition, as `key -> value`.
pub fn rep_metrics(r: &RepResult) -> BTreeMap<String, f64> {
    let mut m = BTreeMap::new();
    for s in &r.steps {
        for (k, v) in step_metrics(s) {
            m.insert(format!("{}.{k}", s.name), v);
        }
    }
    let st = &r.startup;
    // the calc stage runs (`exec-stages`) do not boot the app
    let booted = st.to_first_screen_ms > 0.0;
    if let (true, Some(e)) = (booted, st.exec_ms) {
        m.insert("startup.exec_ms".into(), e);
    }
    for (k, v) in [
        ("data_ms", st.data_ms),
        ("assets_ms", st.assets_ms),
        ("app_new_ms", st.app_new_ms),
        ("on_init_ms", st.on_init_ms),
        ("first_frame_ms", st.first_frame_ms),
        ("to_first_screen_ms", st.to_first_screen_ms),
    ] {
        if booted {
            m.insert(format!("startup.{k}"), v);
        }
    }
    m.insert("total.wall_ms".into(), r.total_ms);
    m.insert("total.cpu_ms".into(), r.cpu_ms);
    m.insert("total.peak_rss_mb".into(), r.peak_rss_mb);
    m
}

/// Summaries over the measured (ok, non-warm-up) repetitions.
pub fn summarize(reps: &[RepResult], warmup: u32) -> BTreeMap<String, Summary> {
    let mut samples: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    for r in reps.iter().filter(|r| r.ok && r.rep >= warmup) {
        for (k, v) in rep_metrics(r) {
            samples.entry(k).or_default().push(v);
        }
    }
    samples
        .into_iter()
        .map(|(k, v)| (k, Summary::of(&v)))
        .collect()
}
