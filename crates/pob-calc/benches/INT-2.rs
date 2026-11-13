//! Gate INT-2 (calc-architecture.md §9.7 "Final (INT-2)", §11 row INT-2): the Rust port's full
//! recalc and power report, single-threaded, in the bench profile (release: thin LTO,
//! `codegen-units = 1`), without the oracle's `probe` feature (the app's build of the calc).
//! The LuaJIT side is `pob-oracle`'s `pob_calc_timing`; `scripts/port/perf_bench.sh` runs both
//! and tabulates them (docs/port/parity/INT-2-perf.md).
//!
//! ```text
//! cargo bench -p pob-calc --bench INT-2 -- [--recalc N] [--warmup N] [--power N] [--json OUT] <build.xml>...
//! ```
//!
//! Per build, the same work as `pob_calc_timing` on the PoB side:
//!
//! * load it (`loadBuildFromXML`, which runs the first `BuildOutput`);
//! * **recalc**: `wipeGlobalCache()` + `CalcsTab:BuildOutput()` (Build.lua:1258-1266),
//!   `--warmup` untimed runs (default 3), then `--recalc` timed runs (default 10);
//! * **power**: `PowerBuilder` over the full tree, Offence/Defence (`powerStat = nil`,
//!   `nodePowerMaxDepth = nil`, `powerMax = nil`), sequential (`PowerOptions::default()`, D12),
//!   `--power` timed runs (default 2).
//!
//! Each run is timed with the wall clock and process CPU time (`CLOCK_PROCESS_CPUTIME_ID`).
//! The output also carries a digest of every main-output leaf and every node power leaf (exact
//! f64 bits, keys sorted) so a release-build optimisation can be checked for bit-identical
//! results on the whole corpus in seconds (`perf_bench.sh --digest`); the oracle gates remain the
//! acceptance (no semantic change, INT-2 row).
#![allow(non_snake_case, clippy::many_single_char_names)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use pob_calc::modules::power::{self, PowerOptions};
use pob_calc::prelude::*;
use pob_gamedata::timeless_lut as lut;

fn cpu_now() -> f64 {
    let t = rustix::time::clock_gettime(rustix::time::ClockId::ProcessCPUTime);
    t.tv_sec as f64 + t.tv_nsec as f64 * 1e-9
}

fn timed(f: impl FnOnce()) -> (f64, f64) {
    let (c0, t0) = (cpu_now(), Instant::now());
    f();
    let wall = t0.elapsed().as_secs_f64();
    (wall, cpu_now() - c0)
}

/// (current RSS, peak RSS) in MB, from /proc/self/status (0 where unavailable).
fn rss_mb() -> (u64, u64) {
    let s = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let field = |k: &str| {
        s.lines()
            .find_map(|l| l.strip_prefix(k))
            .and_then(|v| v.split_whitespace().next())
            .and_then(|v| v.parse::<u64>().ok())
            .map_or(0, |kb| kb / 1024)
    };
    (field("VmRSS:"), field("VmHWM:"))
}

fn json_list(v: &[f64]) -> String {
    let parts: Vec<String> = v.iter().map(|x| format!("{x:.6}")).collect();
    format!("[{}]", parts.join(","))
}

fn key_str(k: &Key) -> String {
    match k {
        Key::S(n) => n.to_string(),
        Key::I(i) => i.to_string(),
        Key::B(b) => b.to_string(),
        Key::F(f) => tostring_num(f.get()),
    }
}

/// Every scalar leaf of `t` under `prefix`, as `key → bits/text`.
fn flat(prefix: &str, t: &Table, out: &mut BTreeMap<String, String>, depth: u32) {
    if depth > 8 {
        return;
    }
    for (k, v) in t.pairs() {
        let p = format!("{prefix}.{}", key_str(&k));
        match v {
            Val::Num(x) => {
                out.insert(
                    p,
                    format!(
                        "{:016x}",
                        if x.is_nan() {
                            f64::NAN.to_bits()
                        } else {
                            x.to_bits()
                        }
                    ),
                );
            }
            Val::Bool(b) => {
                out.insert(p, b.to_string());
            }
            Val::Str(s) => {
                out.insert(p, s.to_string());
            }
            Val::Table(t) => flat(&p, &t, out, depth + 1),
            _ => {}
        }
    }
}

/// FNV-1a 64 over the sorted leaves.
fn digest(m: &BTreeMap<String, String>) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for (k, v) in m {
        for b in k.bytes().chain([0u8]).chain(v.bytes()).chain([1u8]) {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    }
    format!("{h:016x}")
}

fn output_digest(build: &BuildRef) -> (String, usize) {
    let mut m = BTreeMap::new();
    if let Some(o) = build.calcsTab.mainOutput.get() {
        flat("main", &o, &mut m, 0);
    }
    if let Some(o) = build.calcsTab.calcsOutput.get() {
        flat("calcs", &o, &mut m, 0);
    }
    (digest(&m), m.len())
}

fn power_digest(build: &BuildRef) -> (String, usize) {
    let mut m = BTreeMap::new();
    let spec = build.spec.get();
    for (id, node) in spec.nodes.with(LjIntMap::pairs) {
        if let Some(p) = node.extra.table(n!("power")) {
            flat(&format!("nodes.{id}"), &p, &mut m, 0);
        }
    }
    let tree = spec.tree.get();
    for (k, v) in tree.clusterNodeMap.pairs() {
        if let Some(node) = v.obj::<Node>() {
            if let Some(p) = node.extra.table(n!("power")) {
                flat(&format!("cluster.{}", key_str(&k)), &p, &mut m, 0);
            }
        }
    }
    if let Some(p) = power::powerMax(build) {
        flat("powerMax", &p, &mut m, 0);
    }
    (digest(&m), m.len())
}

fn catch<R>(f: impl FnOnce() -> R) -> Result<R, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).map_err(|p| {
        p.downcast_ref::<LuaError>()
            .map(|e| format!("Lua error: {}", e.0))
            .or_else(|| p.downcast_ref::<String>().cloned())
            .or_else(|| p.downcast_ref::<&str>().map(|s| (*s).to_owned()))
            .unwrap_or_else(|| "panic".to_owned())
    })
}

struct Opts {
    recalc: u32,
    warmup: u32,
    power: u32,
}

fn recalc(build: &BuildRef) {
    // Build.lua:1260 wipeGlobalCache(), 1266 self.calcsTab:BuildOutput()
    pob_calc::modules::common::wipeGlobalCache();
    power::BuildOutput(build);
}

fn power_run(build: &BuildRef) {
    power::set_powerStat(build, None);
    power::set_nodePowerMaxDepth(build, None);
    pob_calc::model::build_xml::build_extra(build)
        .get_or_new_table(n!("calcsTab"))
        .set(n!("powerMax"), Val::Nil);
    power::PowerBuilder_run(build, &PowerOptions::default());
}

fn one_build(path: &Path, o: &Opts) -> Result<String, String> {
    let xml = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let name = path
        .file_stem()
        .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
    pob_calc::global_cache::clear_all();
    let t = Instant::now();
    let build = catch(|| pob_calc::model::build_xml::loadBuildFromXML(&xml, pob_gamedata::data()))?;
    let load = t.elapsed().as_secs_f64();
    let (out_digest, out_n) = output_digest(&build);
    for _ in 0..o.warmup {
        catch(|| recalc(&build))?;
    }
    let (mut rw, mut rc) = (Vec::new(), Vec::new());
    for _ in 0..o.recalc {
        let mut r = Ok(());
        let (w, c) = timed(|| r = catch(|| recalc(&build)));
        r?;
        rw.push(w);
        rc.push(c);
    }
    let rss_recalc = rss_mb().0 as f64;
    let (mut pw, mut pc, mut prss) = (Vec::new(), Vec::new(), Vec::new());
    for _ in 0..o.power {
        let mut r = Ok(());
        let (w, c) = timed(|| r = catch(|| power_run(&build)));
        r?;
        pw.push(w);
        pc.push(c);
        prss.push(rss_mb().0 as f64);
    }
    let (pow_digest, pow_n) = if o.power > 0 {
        power_digest(&build)
    } else {
        (String::new(), 0)
    };
    let (rss, peak) = rss_mb();
    let line = format!(
        r#"{{"build":"{name}","engine":"rust","load":{load:.6},"recalc":{{"wall":{},"cpu":{}}},"power":{{"wall":{},"cpu":{}}},"outputDigest":"{out_digest}","outputLeaves":{out_n},"powerDigest":"{pow_digest}","powerLeaves":{pow_n},"rssMB":{rss},"peakRssMB":{peak},"rssAfterRecalcMB":{rss_recalc},"rssAfterPowerMB":{}}}"#,
        json_list(&rw),
        json_list(&rc),
        json_list(&pw),
        json_list(&pc),
        json_list(&prss)
    );
    drop(build);
    pob_calc::global_cache::clear_all();
    Ok(line)
}

fn find_up(rel: &str) -> Option<PathBuf> {
    let mut d = std::env::current_dir().ok()?;
    loop {
        let p = d.join(rel);
        if p.exists() {
            return Some(p);
        }
        if !d.pop() {
            return None;
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let mut builds: Vec<PathBuf> = Vec::new();
    let mut o = Opts {
        recalc: 10,
        warmup: 3,
        power: 2,
    };
    let mut json: Option<PathBuf> = None;
    let num = |v: Option<String>, what: &str| -> Result<u32, String> {
        v.ok_or(format!("{what} N"))?
            .parse()
            .map_err(|e| format!("{what}: {e}"))
    };
    while let Some(a) = args.next() {
        match a.as_str() {
            "--bench" => {} // passed by `cargo bench`
            "--recalc" => o.recalc = num(args.next(), "--recalc")?,
            "--warmup" => o.warmup = num(args.next(), "--warmup")?,
            "--power" => o.power = num(args.next(), "--power")?,
            "--json" => json = Some(args.next().ok_or("--json OUT")?.into()),
            _ if a.starts_with("--") => return Err(format!("unexpected argument {a}")),
            _ => builds.push(a.into()),
        }
    }
    if builds.is_empty() {
        eprintln!("INT-2 bench: no builds given (usage: -- [--recalc N] [--warmup N] [--power N] [--json OUT] <build.xml>...)");
        return Ok(());
    }
    // data.bin (POB_DUMP_DIR, else data/dump/data.bin in an ancestor dir)
    let dump = std::env::var_os("POB_DUMP_DIR")
        .map(PathBuf::from)
        .or_else(|| find_up("data/dump/data.bin").and_then(|p| p.parent().map(Path::to_path_buf)))
        .ok_or("data/dump/data.bin not found (set POB_DUMP_DIR)")?;
    pob_gamedata::init_from_dir(&dump).map_err(|e| format!("data.bin: {e}"))?;
    // Timeless jewel LUTs from PoB's Data/TimelessJewelData (POB_SRC, else .PathOfBuilding/src).
    let src = std::env::var_os("POB_SRC")
        .map(PathBuf::from)
        .or_else(|| find_up(".PathOfBuilding/src"))
        .ok_or("PoB src not found (set POB_SRC)")?;
    let src = std::fs::canonicalize(&src).unwrap_or(src);
    lut::set_jewel_fs(Arc::new(lut::StdJewelFs {
        script_path: src.to_string_lossy().into_owned(),
        writable: false,
    }));
    lut::set_con_printf(|_| {});
    // Lua errors are reported per build, not printed by the default hook.
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if info.payload().downcast_ref::<LuaError>().is_none() {
            prev(info);
        }
    }));
    let mut lines = Vec::new();
    for b in &builds {
        let line = one_build(b, &o).unwrap_or_else(|e| {
            let name = b
                .file_stem()
                .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
            format!(r#"{{"build":"{name}","engine":"rust","error":{:?}}}"#, e)
        });
        if json.is_none() {
            println!("{line}");
        }
        lines.push(line);
    }
    if let Some(p) = json {
        std::fs::write(&p, format!("[{}]\n", lines.join(",\n")))
            .map_err(|e| format!("{}: {e}", p.display()))?;
    }
    Ok(())
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            std::process::ExitCode::FAILURE
        }
    }
}
