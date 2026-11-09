//! Gate G1a (calc doc §9.7, after MS-B): ModStore query speed against LuaJIT.
//!
//! ```text
//! scripts/port/cargo.sh bench -p pob-oracle --bench MS-B            # all57
//! MS_B_BUILDS=a,b MS_B_LIMIT=n MS_B_LUA_ITERS=200 …                 # knobs
//! ```
//!
//! 1. For every all57 build, PoB is booted as it ships (JIT on, like `pob_timing`; the oracle
//!    lock held), the build is loaded and calculated, and the `pob_timing` measurement runs on
//!    the main env at `perform:out` (`calcsTab.mainEnv`): for every mod name of
//!    `env.player.modDB` (sorted), with cfg = nil and the main skill's `skillCfg`,
//!    `Sum("BASE")`, `Sum("INC")`, `More`, `Flag`, each `N` times after a warm-up pass
//!    (mean ns per query). The same process then dumps the stores, actors and cfgs
//!    (`corpus/MS-B/l3_full.lua` with `__msb_no_queries`), and a checksum of the query results.
//! 2. Rust rebuilds the stores (`corpus/MS-B/rebuild.rs`), checks the same checksums, and
//!    measures the same queries with criterion: one iteration runs every (build, name) query of
//!    one (cfg, kind) once; ns/query = mean iteration time / queries per iteration.
//! 3. `dhat`: after one warm-up pass, `Sum`/`More`/`Flag` over every name of every build (cfg
//!    nil and skillCfg) must allocate nothing (names are interned `Name`s, i.e. `n!` constants).
//!
//! Gate: for each (cfg, kind) the Rust ns/query ≤ LuaJIT's (query-weighted mean over the
//! builds), and zero allocations. The table is printed; the process exits non-zero on failure.
#![allow(non_snake_case)]

use std::cell::RefCell;
use std::hint::black_box;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use criterion::Criterion;
use pob_calc::prelude::*;
use pob_oracle::headless::{Engine, Ret};
use pob_oracle::jit::JitPob;
use pob_oracle::oracle::{find_parity_dir, find_pob_src, QUIET_PRELUDE};
use pob_oracle::runner::OracleLock;
use serde_json::Value as J;

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

#[path = "../corpus/MS-B/rebuild.rs"]
mod rebuild;

/// `pob_timing`'s measurement (crates/pob-oracle/src/bin/pob_timing.rs), plus a checksum of the
/// results (sum of the numeric results, count of `true` flags) per (cfg, kind).
const TIMING_LUA: &str = r#"
local iters = __timing_iters
local env = build.calcsTab.mainEnv
local modDB = env.player.modDB
local skill = env.player.mainSkill
local cfgs = { { "nil", nil } }
if skill and skill.skillCfg then cfgs[2] = { "skillCfg", skill.skillCfg } end
local names = { }
for name in pairs(modDB.mods) do names[#names + 1] = name end
table.sort(names)
local clock = os.clock
local kinds = {
	{ "SumBASE", function(cfg, name) return modDB:Sum("BASE", cfg, name) end },
	{ "SumINC", function(cfg, name) return modDB:Sum("INC", cfg, name) end },
	{ "More", function(cfg, name) return modDB:More(cfg, name) end },
	{ "Flag", function(cfg, name) return modDB:Flag(cfg, name) end },
}
local out = { }
for _, c in ipairs(cfgs) do
	for _, k in ipairs(kinds) do
		local f, cfg = k[2], c[2]
		local vals, errs = { }, 0
		for i, name in ipairs(names) do -- warm-up (and the results, checked by the Rust side)
			local ok, v = pcall(f, cfg, name)
			if not ok then errs = errs + 1; vals[i] = '"err"'
			elseif type(v) == "boolean" then vals[i] = tostring(v)
			elseif v == nil then vals[i] = "false"
			elseif v ~= v or v == math.huge or v == -math.huge then vals[i] = '"' .. tostring(v) .. '"'
			else vals[i] = string.format("%.17g", v) end
		end
		local t0 = clock()
		if errs == 0 then
			for _ = 1, iters do
				for _, name in ipairs(names) do f(cfg, name) end
			end
		end
		local dt = clock() - t0
		out[#out + 1] = string.format('{"cfg":"%s","kind":"%s","queries":%d,"nsPerQuery":%.3f,"errs":%d,"vals":[%s]}',
			c[1], k[1], #names * iters, dt * 1e9 / (#names * iters), errs, table.concat(vals, ","))
	end
end
return '{"names":' .. #names .. ',"iters":' .. iters .. ',"results":[' .. table.concat(out, ",") .. "]}"
"#;

const KINDS: [&str; 4] = ["SumBASE", "SumINC", "More", "Flag"];
const CFGS: [&str; 2] = ["nil", "skillCfg"];

struct Set {
    build: rebuild::Build,
    names: Vec<Name>,
    /// index into build.cfgs for "nil" / "skillCfg" (None: the build has no main skillCfg)
    cfg: [Option<usize>; 2],
}

#[inline]
fn query(store: &ModStore, cfg: Option<&ModCfg>, kind: usize, name: Name) -> f64 {
    let names = [name];
    match kind {
        0 => store.Sum(ModType::BASE, cfg, &names),
        1 => store.Sum(ModType::INC, cfg, &names),
        2 => store.More(cfg, &names),
        _ => f64::from(u8::from(store.Flag(cfg, &names))),
    }
}

fn run_all(sets: &[Set], ci: usize, kind: usize) -> f64 {
    let mut acc = 0.0;
    for s in sets {
        let Some(c) = s.cfg[ci] else { continue };
        let cfg = s.build.cfgs[c].as_deref();
        let store = &s.build.stores[0];
        for &n in &s.names {
            acc += black_box(query(store, black_box(cfg), kind, black_box(n)));
        }
    }
    acc
}

fn stub_hits() -> u64 {
    pob_calc::stub::stub_report().iter().map(|(_, c)| *c).sum()
}

fn corpus() -> Vec<PathBuf> {
    let parity = find_parity_dir().expect(".parity not found");
    let mut v: Vec<PathBuf> = std::fs::read_dir(parity.join("builds"))
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "xml"))
        .collect();
    v.sort();
    if let Ok(list) = std::env::var("MS_B_BUILDS") {
        let want: Vec<&str> = list.split(',').map(str::trim).collect();
        v.retain(|p| want.contains(&p.file_stem().unwrap().to_str().unwrap()));
    }
    if let Some(n) = std::env::var("MS_B_LIMIT")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
    {
        v.truncate(n);
    }
    v
}

fn main() {
    let src = find_pob_src().expect("PoB checkout not found");
    assert!(rebuild::init_data(), "data/dump not found");
    let iters: u32 = std::env::var("MS_B_LUA_ITERS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(200);
    // LuaJIT: total ns and queries per (cfg, kind); checksums per build
    let mut lua_ns = [[0.0f64; 4]; 2];
    let mut lua_q = [[0u64; 4]; 2];
    let mut sets = Vec::new();
    let mut checks = 0;
    let mut left_out = 0;
    for xml in corpus() {
        let stem = xml.file_stem().unwrap().to_string_lossy().into_owned();
        let text = std::fs::read_to_string(&xml).unwrap();
        let (timing, dump) = {
            let _lock = OracleLock::acquire().expect("oracle lock");
            let mut pob = JitPob::boot_with(&src, Some(QUIET_PRELUDE))
                .unwrap_or_else(|e| panic!("boot: {e}"));
            pob.load_build_xml(&text, &stem)
                .unwrap_or_else(|e| panic!("{stem}: load: {e}"));
            let t = pob
                .run(&format!("__timing_iters = {iters}\n{TIMING_LUA}"))
                .unwrap_or_else(|e| panic!("{stem}: timing: {e}"));
            let d = pob
                .run(&format!("__msb_no_queries = true\n{}", rebuild::SCRIPT))
                .unwrap_or_else(|e| panic!("{stem}: dump: {e}"));
            let s = |r: &[Ret]| match r.first() {
                Some(Ret::Str(s)) => s.clone(),
                other => panic!("{stem}: script returned {other:?}"),
            };
            (s(&t), s(&d))
        };
        let timing: J = serde_json::from_str(&timing).unwrap();
        let doc: J = serde_json::from_str(&dump).unwrap();
        let build = rebuild::load(&doc);
        let names: Vec<Name> = doc["stores"][0]["buckets"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| Name::from(b[0].as_str().unwrap()))
            .collect();
        assert_eq!(
            names.len() as u64,
            timing["names"].as_u64().unwrap(),
            "{stem}: name sets differ"
        );
        let find = |n: &str| build.cfg_names.iter().position(|x| x == n);
        // Names with a mod the Rust model cannot decode (CCR-0014 §3, warcry tags) are left
        // out of the Rust side (their LuaJIT timing is kept: a handful of names).
        let bad = &build.undecodable_names[0];
        let keep: Vec<bool> = names.iter().map(|n| !bad.contains(n)).collect();
        let mut set = Set {
            cfg: [find("nil"), find("skillCfg")],
            names: Vec::new(),
            build,
        };
        // names whose query reaches IT-B's Item:FindModifierSubstring stub
        let mut pending = std::collections::HashSet::new();
        for r in timing["results"].as_array().unwrap() {
            let ci = CFGS.iter().position(|c| *c == r["cfg"]).unwrap();
            let ki = KINDS.iter().position(|k| *k == r["kind"]).unwrap();
            assert_eq!(r["errs"], 0, "{stem}: PoB raised in a timed query");
            lua_ns[ci][ki] += r["nsPerQuery"].as_f64().unwrap() * r["queries"].as_f64().unwrap();
            lua_q[ci][ki] += r["queries"].as_u64().unwrap();
            // the same results in Rust, name by name
            let c = set.cfg[ci].expect("cfg present in Lua but not in the dump");
            let cfg = set.build.cfgs[c].as_deref();
            let store = &set.build.stores[0];
            let vals = r["vals"].as_array().unwrap();
            for (i, &n) in names.iter().enumerate() {
                if !keep[i] {
                    continue;
                }
                let stub0 = stub_hits();
                let v = query(store, cfg, ki, n);
                if stub_hits() != stub0 {
                    // Item:FindModifierSubstring is still IT-B's stub: not comparable yet
                    pending.insert(n);
                    continue;
                }
                let ok = match &vals[i] {
                    J::Bool(b) => (v != 0.0) == *b,
                    J::String(s) => match s.as_str() {
                        "nan" | "-nan" => v.is_nan(),
                        "inf" => v == f64::INFINITY,
                        "-inf" => v == f64::NEG_INFINITY,
                        _ => false,
                    },
                    x => x.as_f64().is_some_and(|x| x == v),
                };
                assert!(
                    ok,
                    "{stem}: {} {} {n}: lua {} rust {v}",
                    CFGS[ci], KINDS[ki], vals[i]
                );
                checks += 1;
            }
        }
        set.names = names
            .iter()
            .zip(&keep)
            .filter(|(n, k)| **k && !pending.contains(*n))
            .map(|(n, _)| *n)
            .collect();
        eprintln!(
            "{stem}: {} names ({} left out: {} with an undecodable mod, {} pending IT-B), {} results checked so far",
            names.len(),
            names.len() - set.names.len(),
            keep.iter().filter(|k| !**k).count(),
            pending.len(),
            checks
        );
        left_out += names.len() - set.names.len();
        sets.push(set);
    }

    // --- dhat: no allocation in Sum / More / Flag ---
    for ci in 0..2 {
        for ki in 0..4 {
            run_all(&sets, ci, ki); // warm-up (memo tables)
        }
    }
    let allocs = {
        let _p = dhat::Profiler::builder().testing().build();
        let before = dhat::HeapStats::get();
        for ci in 0..2 {
            for ki in 0..4 {
                black_box(run_all(&sets, ci, ki));
            }
        }
        let after = dhat::HeapStats::get();
        if after.total_blocks != before.total_blocks {
            // name the allocating queries (and the tag types of the mods they read)
            let mut shown = 0;
            for s in &sets {
                for ci in 0..2 {
                    let Some(c) = s.cfg[ci] else { continue };
                    let cfg = s.build.cfgs[c].as_deref();
                    let store = &s.build.stores[0];
                    for &n in &s.names {
                        for ki in 0..4 {
                            let b0 = dhat::HeapStats::get().total_blocks;
                            black_box(query(store, cfg, ki, n));
                            let d = dhat::HeapStats::get().total_blocks - b0;
                            if d > 0 && shown < 40 {
                                shown += 1;
                                let tags: Vec<String> = store
                                    .mods_named(n)
                                    .iter()
                                    .flat_map(|m| m.tags.iter().map(|t| format!("{:?}", t.r#type)))
                                    .collect();
                                eprintln!(
                                    "  dhat: {d} allocation(s): {} {} {n} tags {tags:?}",
                                    CFGS[ci], KINDS[ki]
                                );
                            }
                        }
                    }
                }
            }
        }
        (
            after.total_blocks - before.total_blocks,
            after.total_bytes - before.total_bytes,
        )
    };
    let nq: u64 = sets
        .iter()
        .map(|s| s.names.len() as u64 * s.cfg.iter().filter(|c| c.is_some()).count() as u64 * 4)
        .sum();
    eprintln!(
        "dhat: {} allocations ({} bytes) over {nq} Sum/More/Flag queries",
        allocs.0, allocs.1
    );

    // --- criterion ---
    let mut c = Criterion::default()
        .warm_up_time(Duration::from_millis(500))
        .measurement_time(Duration::from_secs(
            std::env::var("MS_B_MEASURE_SECS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(3),
        ))
        .sample_size(20)
        .configure_from_args();
    let mut rust_ns = [[0.0f64; 4]; 2];
    let mut group = c.benchmark_group("G1a");
    for ci in 0..2 {
        for ki in 0..4 {
            let per_iter: u64 = sets
                .iter()
                .filter(|s| s.cfg[ci].is_some())
                .map(|s| s.names.len() as u64)
                .sum();
            let acc = RefCell::new((Duration::ZERO, 0u64));
            group.bench_function(format!("{}-{}", CFGS[ci], KINDS[ki]), |b| {
                // `pob_timing`'s loop shape: per build, `n` passes over its names (the build's
                // stores stay hot, as in the one-build LuaJIT process); one iteration = one pass
                // over every build.
                b.iter_custom(|n| {
                    let t0 = Instant::now();
                    for s in &sets {
                        for _ in 0..n {
                            black_box(run_all(std::slice::from_ref(s), ci, ki));
                        }
                    }
                    let dt = t0.elapsed();
                    let mut a = acc.borrow_mut();
                    a.0 += dt;
                    a.1 += n;
                    dt
                });
            });
            let (d, n) = *acc.borrow();
            rust_ns[ci][ki] = d.as_nanos() as f64 / (n as f64 * per_iter as f64);
        }
    }
    group.finish();

    // --- gate ---
    let mut fail = allocs.0 != 0;
    eprintln!(
        "\nG1a over {} builds, {} results checked against PoB, {} names left out of the Rust side (ns/query, query-weighted mean):",
        sets.len(),
        checks,
        left_out
    );
    eprintln!(
        "  {:<10} {:<8} {:>10} {:>10} {:>7}",
        "cfg", "query", "LuaJIT", "Rust", "ratio"
    );
    for ci in 0..2 {
        for ki in 0..4 {
            let lua = lua_ns[ci][ki] / lua_q[ci][ki] as f64;
            let rust = rust_ns[ci][ki];
            let ok = rust <= lua;
            fail |= !ok;
            eprintln!(
                "  {:<10} {:<8} {:>10.1} {:>10.1} {:>7.3} {}",
                CFGS[ci],
                KINDS[ki],
                lua,
                rust,
                rust / lua,
                if ok { "ok" } else { "FAIL" }
            );
        }
    }
    eprintln!(
        "  dhat: {} allocations in Sum/More/Flag {}",
        allocs.0,
        if allocs.0 == 0 { "ok" } else { "FAIL" }
    );
    if fail {
        eprintln!("G1a: FAILED");
        std::process::exit(1);
    }
    eprintln!("G1a: passed");
}
