//! `pob-prof`: the profiling suite of regular use (PROF1). Owner: PROF1.
//!
//! It replays scenarios of what a user does (open builds, allocate nodes, hover the tree, swap
//! items, edit gems, toggle config, read Calcs, the power report, Compare, import, save) on the
//! real `pob_app::PobApp` with the Rust calc, headless, and measures frame times, recalc latency,
//! build load and start-up time, allocations and peak RSS. Runs are repeated in fresh processes,
//! summarised with medians and their dispersion, written as JSON, and compared against a
//! committed baseline. It depends on no PoB checkout code, no LuaJIT and no oracle crate: only on
//! the product crates and the files the product itself reads.
//!
//! How to use it: docs/perf/PROFILING.md. Modules:
//! - [`scenario`]: the scenario files (`.pobscript` plus the suite's directives);
//! - [`driver`]: one scenario on the app, with the per-step measurements;
//! - [`calc_timing`]: the timing `CalcFacade` (recalc latency as the app sees it);
//! - [`stages`]: the calc's stages timed from outside (initEnv, perform, buildOutput, …);
//! - [`render`]: none / CPU raster / offscreen wgpu;
//! - [`alloc`]: the counting allocator;
//! - [`results`], [`stats`], [`compare`]: the results file, statistics, baseline comparison;
//! - [`hotspots`]: `perf script` folding and hotspot summaries for flamegraphs;
//! - [`paths`], [`sysinfo`]: inputs and machine state.

#![allow(clippy::many_single_char_names)]

pub mod alloc;
pub mod calc_timing;
pub mod compare;
pub mod driver;
pub mod hotspots;
pub mod paths;
pub mod render;
pub mod results;
pub mod scenario;
pub mod stages;
pub mod stats;
pub mod sysinfo;
