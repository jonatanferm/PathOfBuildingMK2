//! LuaJIT timings of ModStore queries: the baseline of gate G1a
//! (calc-architecture.md §9.7: Rust ns/query ≤ LuaJIT ns/query of the same
//! query).
//!
//! ```text
//! pob_timing <build.xml> [--iters N] [--json OUT] [--pob SRC]
//! ```
//!
//! Loads the build in PoB (unpatched), takes the main env after
//! `calcs.perform` (`calcsTab.mainEnv`), and times, for every mod name of
//! `env.player.modDB` (names sorted) with cfg = nil and the main skill's
//! `skillCfg`: `Sum("BASE")`, `Sum("INC")`, `More`, `Flag`. Each query
//! runs `N` times (default 200) after one warm-up pass; the result is the
//! mean ns per query per kind, plus the number of queries.

use std::path::PathBuf;
use std::process::ExitCode;

use pob_oracle::headless::Engine;
use pob_oracle::oracle::{find_pob_src, QUIET_PRELUDE};
use pob_oracle::runner::{write_atomic, OracleLock};

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
		for _, name in ipairs(names) do f(cfg, name) end -- warm-up
		local t0 = clock()
		for _ = 1, iters do
			for _, name in ipairs(names) do f(cfg, name) end
		end
		local dt = clock() - t0
		out[#out + 1] = string.format('{"cfg":"%s","kind":"%s","queries":%d,"nsPerQuery":%.3f}',
			c[1], k[1], #names * iters, dt * 1e9 / (#names * iters))
	end
end
return '{"build":"' .. tostring(__pob_name) .. '","names":' .. #names .. ',"iters":' .. iters .. ',"results":[' .. table.concat(out, ",") .. "]}"
"#;

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let mut build: Option<PathBuf> = None;
    let mut iters = 200u32;
    let mut json: Option<PathBuf> = None;
    let mut pob: Option<PathBuf> = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--iters" => {
                iters = args
                    .next()
                    .ok_or("--iters N")?
                    .parse()
                    .map_err(|e| format!("{e}"))?;
            }
            "--json" => json = Some(args.next().ok_or("--json OUT")?.into()),
            "--pob" => pob = Some(args.next().ok_or("--pob SRC")?.into()),
            _ if build.is_none() => build = Some(a.into()),
            _ => return Err(format!("unexpected argument {a}")),
        }
    }
    let build =
        build.ok_or("usage: pob_timing <build.xml> [--iters N] [--json OUT] [--pob SRC]")?;
    let src = match pob {
        Some(p) if p.join("src/HeadlessWrapper.lua").is_file() => p.join("src"),
        Some(p) => p,
        None => find_pob_src().ok_or("PoB not found: pass --pob or set POB_SRC")?,
    };
    let xml = std::fs::read_to_string(&build).map_err(|e| format!("{}: {e}", build.display()))?;
    let name = build
        .file_stem()
        .map_or_else(|| "build".to_owned(), |s| s.to_string_lossy().into_owned());
    let _lock = OracleLock::acquire().map_err(|e| e.0)?;
    // Timing baseline: PoB as it ships, JIT on (not an oracle parity run, so not
    // `boot_oracle`; CCR-0001).
    let mut pob = pob_oracle::jit::JitPob::boot_with(&src, Some(QUIET_PRELUDE))?;
    pob.load_build_xml(&xml, &name)?;
    let r = pob.run(&format!("__timing_iters = {iters}\n{TIMING_LUA}"))?;
    let text = r.first().map(|r| r.text().to_owned()).unwrap_or_default();
    match json {
        Some(p) => write_atomic(&p, &text).map_err(|e| e.0)?,
        None => println!("{text}"),
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
