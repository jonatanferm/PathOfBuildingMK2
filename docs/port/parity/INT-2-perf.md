# INT-2 performance: power report and full recalc vs PoB under LuaJIT

Unit INT-2 (calc doc §11 row INT-2, §9.7 "Final (INT-2)"), measured 2026-11-12 on rust-port
`fa733f3`. Gate: the power report time ≤ PoB's LuaJIT time and the full recalc time ≤ PoB's
LuaJIT time, both single-threaded, release build, with the JIT on.

## 1. Result

**Gate passed on every build measured, with no code change to the calc.**

- Gate set (10 builds, §3): the Rust power report takes **0.48-0.85** of LuaJIT's CPU time
  (total 57.1 s vs 88.5 s, 0.65). The full recalc takes **0.64-0.82** of LuaJIT's (total
  0.437 s vs 0.585 s, 0.75).
- 4 more builds, the ones with the highest Rust/LuaJIT power ratio in the unpinned corpus
  survey: power 0.70-0.83, recalc 0.81-0.88.
- Unpinned full-corpus survey (57 builds, ranking only, §4): recalc ≤ LuaJIT on 57/57 (total
  0.73). Power ≤ LuaJIT on 56/57 (total 0.61). The exception, allflame_exp_012 (1.07), comes from
  two runs at different times without pinning. Pinned, it is 0.80 (§3.2).
- No optimisation was applied. The profile (§6) has no hot spot in INT-2's files (`calcs.rs`,
  `power.rs`), and no symbol above 3.6 % self time. It is spread over the table, interning and
  mod-store primitives of other units. With the gate met, none of that work was justified.
- Finding: an `Rc` cycle leaks memory. Each minion skill's `summonSkill` back-pointer closes a
  cycle, so every dropped env leaks its minion skills. One power report keeps up to 2.25 GB, and
  a recalc about 12 MB. This is **CCR-0135** (§5). The proposed fix gives bit-identical results
  on the gate set and the gate still passes with it (power 0.54-0.85, recalc 0.69-0.88).
- No semantic change: CA, D13, PWR and AS are exact on the full corpus (§7).

## 2. Method

**What is timed.** The two sides run the same work.

| | PoB (`pob-oracle` bin `pob_calc_timing`) | Rust (`pob-calc` bench `INT-2`) |
|---|---|---|
| load | `main:SetMode("BUILD", …)` + one `OnFrame` (not gated) | `loadBuildFromXML` (not gated) |
| full recalc | `wipeGlobalCache()` + `calcsTab:BuildOutput()` (the calc part of `Build:OnFrame` on `buildFlag`, Build.lua:1258-1266: MAIN + CALCS `calcs.buildOutput`, which includes the cost-warning `buildActiveSkill` pass, and `getMiscCalculator`) | `common::wipeGlobalCache()` + `power::BuildOutput` |
| power report | `powerStat = nil` (Offence/Defence), `nodePowerMaxDepth = nil` (whole tree), `powerMax = nil`, then `calcsTab:PowerBuilder()` called directly (outside a coroutine it never yields; the work is the coroutine's) | the same settings, `power::PowerBuilder_run(…, PowerOptions::default())` (sequential, D12) |

Per process: load, 3 untimed recalcs, 10 timed recalcs, 1 timed power report. **3 rounds** use
fresh processes, and the two sides of a build run back to back in each round. The summary takes
the **minimum process CPU time** (`CLOCK_PROCESS_CPUTIME_ID`) over all samples, which is the
least disturbed run on a shared machine. Wall time and medians are recorded too. Only one power
report runs per process, because a second one does not fit in 3 GB until CCR-0135 is applied (§5).

**Engines.**
- PoB v2.67.2 @16de4b82d, unpatched, in-process through `mlua` 0.10 with the vendored LuaJIT
  2.1 (`luajit-src` 210.5.12+a4f56a4). **JIT on**: `JitPob::boot_with`, not the oracle's
  `jit.off()`. PoB's own `Launch.lua` settings apply (`jit.opt.start('maxtrace=4000',
  'maxmcode=8192')`, `collectgarbage("setpause", 400)`).
- Rust: rustc 1.98.1. Bench profile = release: `lto = "thin"`, `codegen-units = 1`. It builds
  inside `cargo.sh`'s 5 GB cap, so no reduced bench profile was needed. The calc is built
  **without** the oracle's `probe` feature, as the app builds it.

**Host.** Apple M1 Pro (2 Icestorm efficiency cores, CPUs 0-1 at 2.06 GHz; 6 Firestorm
performance cores, CPUs 2-7 at 3.04 GHz), Asahi Linux 7.1.13, 16 GB. The machine is shared with
other agents (load average 7-10 during the runs).

**Pinning.** Both sides are pinned to the performance cores (`taskset -c 2-7`, the script's
default: the CPUs with the highest max frequency). The first, unpinned runs were dominated by
the scheduler moving a run onto an efficiency core. perf showed one Rust power report with about
20 % of its cycles on Icestorm, and the same binary on the same build varied by up to 1.6x
(allflame_exp_013 with the CCR-0135 patch: 9.1 s pinned, 13.2-14.5 s unpinned).

**Reproduce.**

```sh
scripts/port/perf_bench.sh                       # the gate set (10 builds, 3 rounds)
scripts/port/perf_bench.sh --builds a,b --rounds 1
scripts/port/perf_bench.sh --batch --builds all --power 0 --recalc 5   # corpus recalc survey
scripts/port/perf_bench.sh --digest              # Rust only: output/power digests of every build
```

Each Rust run is one `cargo.sh bench -p pob-calc --bench INT-2` (a run slot, a memory-capped
scope). Each PoB run is one `pob_calc_timing` process (the machine-wide oracle lock, a 3 GB
scope). Results go to `target/int2-perf/<stamp>/{lua,rust}.jsonl` and `summary.txt`. The Rust
lines also carry an output digest and a power digest (exact f64 bits of every main/CALCS output
leaf and node power leaf), so a later optimisation can be checked bit for bit on the whole corpus
in minutes, before the oracle gates run.

## 3. Gate results (pinned, 3 rounds, min CPU time)

### 3.1 Gate set

Build set (from the unpinned survey, §4): allflame_exp_013, 051, 030, 007, 039 and
upstream_Dual_Wield_Cospris_CoC are the 1st-4th, 7th and 9th heaviest builds by PoB power-report
time (8.6-13.3 s). allflame_exp_043 is the heaviest by PoB recalc time. allflame_exp_000 and
allflame_exp_011 had the 3rd- and 4th-highest Rust/LuaJIT power ratio in the survey (the higher
two and the next two are in §3.2); allflame_exp_000 is also a PWR gate build. allflame_exp_014 is
the PWR gate's cluster-jewel minion build.

| build | recalc LuaJIT ms (min/med) | recalc Rust ms (min/med) | ratio | power LuaJIT s (min/med) | power Rust s (min/med) | ratio | peak RSS MB LuaJIT / Rust |
|---|---|---|---|---|---|---|---|
| allflame_exp_013 | 49.0 / 54.2 | 38.7 / 39.9 | 0.79 | 13.34 / 14.57 | 8.63 / 9.00 | 0.65 | 882 / 1902 |
| allflame_exp_051 | 56.8 / 63.5 | 42.7 / 44.3 | 0.75 | 12.06 / 13.36 | 7.81 / 8.03 | 0.65 | 901 / 1616 |
| allflame_exp_030 | 75.4 / 87.0 | 48.4 / 49.7 | 0.64 | 10.53 / 10.98 | 5.08 / 5.24 | 0.48 | 800 / 1738 |
| allflame_exp_039 | 63.7 / 83.8 | 47.2 / 48.6 | 0.74 | 9.81 / 12.33 | 6.79 / 6.82 | 0.69 | 1000 / 2770 |
| upstream_Dual_Wield_Cospris_CoC | 53.1 / 57.4 | 42.0 / 43.0 | 0.79 | 8.60 / 8.89 | 5.69 / 5.71 | 0.66 | 871 / 1402 |
| allflame_exp_043 | 63.6 / 71.6 | 47.2 / 49.5 | 0.74 | 8.16 / 8.37 | 4.85 / 5.05 | 0.59 | 868 / 2132 |
| allflame_exp_007 | 65.8 / 75.5 | 47.2 / 49.6 | 0.72 | 7.51 / 7.87 | 5.06 / 5.93 | 0.67 | 843 / 2033 |
| allflame_exp_014 | 58.8 / 74.8 | 48.3 / 49.6 | 0.82 | 6.82 / 7.32 | 4.75 / 4.77 | 0.70 | 860 / 2045 |
| allflame_exp_011 | 53.9 / 81.9 | 40.9 / 42.4 | 0.76 | 6.39 / 7.33 | 3.91 / 4.02 | 0.61 | 794 / 1140 |
| allflame_exp_000 | 44.9 / 54.6 | 34.0 / 37.5 | 0.76 | 5.29 / 5.42 | 4.51 / 4.52 | 0.85 | 1055 / 490 |

recalc: total min CPU LuaJIT 0.585 s, Rust 0.437 s, ratio 0.75. Power: LuaJIT 88.5 s, Rust
57.1 s, ratio 0.65. Every build ≤ LuaJIT on both. The Rust digests are the same in all 3 rounds.
Peak RSS is the whole process (PoB includes its booted UI state). The Rust peaks above ~500 MB
are the CCR-0135 leak.

### 3.2 Highest-ratio builds from the survey

| build | recalc LuaJIT ms (min/med) | recalc Rust ms (min/med) | ratio | power LuaJIT s (min/med) | power Rust s (min/med) | ratio | peak RSS MB LuaJIT / Rust |
|---|---|---|---|---|---|---|---|
| allflame_exp_032 | 35.1 / 37.4 | 28.5 / 29.5 | 0.81 | 5.05 / 5.25 | 4.08 / 4.16 | 0.81 | 789 / 439 |
| allflame_exp_016 | 23.9 / 26.7 | 20.9 / 21.1 | 0.88 | 4.16 / 4.25 | 3.45 / 3.63 | 0.83 | 727 / 430 |
| allflame_exp_012 | 32.7 / 36.8 | 26.7 / 27.1 | 0.82 | 4.05 / 4.30 | 3.23 / 3.26 | 0.80 | 721 / 437 |
| upstream_OccVortex | 29.8 / 31.5 | 25.4 / 26.0 | 0.85 | 3.78 / 3.94 | 2.63 / 2.74 | 0.70 | 782 / 425 |

The margin is smallest on light builds without minions (0.80-0.88). On the heavy minion and
mirage builds the Rust lead is largest (0.48-0.70), partly because of the leak (§5).

## 4. Corpus surveys (unpinned, for ranking only)

- **Recalc**, all 57 builds, one process per side (`--batch`, 5 timed recalcs): Rust ≤ LuaJIT on
  57/57, ratio 0.49-0.88 (median 0.75), total 1.771 s vs 2.423 s (0.73).
- **Power**, all 57 builds, 1 report each. LuaJIT ran in one batch process. Rust ran one process
  per build, because the batch process was OOM-killed at 5 GB after 13 builds (§5). Rust ≤ LuaJIT
  on 56/57, ratio 0.42-1.07 (median 0.64), total 248.8 s vs 408.3 s (0.61). The 1.07 is
  allflame_exp_012, from separate unpinned runs; pinned it is 0.80 (§3.2). The next highest ratios
  are upstream_OccVortex 0.93, allflame_exp_000 0.90, allflame_exp_011 0.84, allflame_exp_016
  0.75 and allflame_exp_032 0.74. All were re-measured pinned (§3).

## 5. Finding: summonSkill `Rc` cycle (CCR-0135)

`ActiveSkill.summonSkill` is a strong `Rc` (frozen contract, `active_skill.rs:80`). The summoning
skill owns its minion, the minion owns its skills, and each of those skills points back at the
summoning skill. Calc doc §3.3 rule 4 forbids this cycle. Lua's GC collects it; `Rc` never
frees it. A diagnosis run (env dropped, `GlobalCache` cleared, `Weak`s checked) shows that
exactly the skills with a `minion` and their minion actors survive. With `summonSkill` cut,
nothing survives.

- RSS growth of one power report: more than 300 MB on 32/57 corpus builds, up to 2.25 GB
  (allflame_exp_039). The memory never comes back. Two reports of allflame_exp_013 or
  allflame_exp_039 exceed 3 GB, and a 13-build batch process was OOM-killed at 5 GB.
- Recalc: 100 recalcs of allflame_exp_039 end at 1,598 MB RSS (vs 349 MB with the fix).

The proposed fix makes `summonSkill` a `Weak` (CCR-0135 §3). It touches 7 call sites in core,
setup, perform and offence plus probe and ui-oracle, all files of other units, so it is not
applied here. Measured with the fix applied temporarily (pinned, 3 rounds, same session as §3.1;
the LuaJIT column is §3.1's):

| build | recalc ms LuaJIT / Rust / Rust+CCR-0135 | power s LuaJIT / Rust / Rust+CCR-0135 | power ratio Rust+CCR-0135 / LuaJIT | RSS after power MB Rust / Rust+CCR-0135 | digests equal |
|---|---|---|---|---|---|
| allflame_exp_013 | 49.0 / 38.7 / 40.8 | 13.34 / 8.63 / 9.07 | 0.68 | 1902 / 484 | yes |
| allflame_exp_051 | 56.8 / 42.7 / 44.6 | 12.06 / 7.81 / 8.51 | 0.71 | 1616 / 484 | yes |
| allflame_exp_030 | 75.4 / 48.4 / 52.0 | 10.53 / 5.08 / 5.69 | 0.54 | 1738 / 445 | yes |
| allflame_exp_039 | 63.7 / 47.2 / 51.2 | 9.81 / 6.79 / 7.86 | 0.80 | 2770 / 690 | yes |
| upstream_Dual_Wield_Cospris_CoC | 53.1 / 42.0 / 44.2 | 8.60 / 5.69 / 6.38 | 0.74 | 1402 / 434 | yes |
| allflame_exp_043 | 63.6 / 47.2 / 50.4 | 8.16 / 4.85 / 5.17 | 0.63 | 2132 / 447 | yes |
| allflame_exp_007 | 65.8 / 47.2 / 50.2 | 7.51 / 5.06 / 5.21 | 0.69 | 2033 / 451 | yes |
| allflame_exp_014 | 58.8 / 48.3 / 51.8 | 6.82 / 4.75 / 5.63 | 0.83 | 2045 / 447 | yes |
| allflame_exp_011 | 53.9 / 40.9 / 43.2 | 6.39 / 3.91 / 4.12 | 0.64 | 1140 / 441 | yes |
| allflame_exp_000 | 44.9 / 34.0 / 34.4 | 5.29 / 4.51 / 4.51 | 0.85 | 490 / 489 | yes |

"Digests equal": the output and power digests of every round equal the unpatched calc's. The
fix costs time (0 %…+19 % power, +1 %…+8 % recalc), because the leak saved the frees. Even so,
the gate still passes with it on every gate build: power ≤ 0.85, recalc ≤ 0.88. Re-run
`perf_bench.sh` after CCR-0135 merges.

## 6. Profile (perf, release bench binary, power report)

`perf record -g` of one sequential power report: allflame_exp_013 and upstream_OccVortex
(unpinned), allflame_exp_012 (pinned). The shape is the same on all three:

- `misc_calculator_call` takes 80-90 % of the samples: `initEnv` 33-35 % and `perform` 47-71 %.
  On mirage builds, `calc_mirages::mirages` alone is 44 % (it re-runs `copyActiveSkill` and
  `perform`). These are the calc's own Lua-faithful passes, as in PoB.
- `power.rs` (PowerBuilder, CalculatePowerStat, the cache) has under 0.05 % self time, and
  `calcs.rs` only orchestrates. Neither owned file has anything to optimise.
- `cacheData` → `set_cachedData` → `drop(Env)`: 6-7 %. Overwriting a `GlobalCache` entry frees
  the previous env. In Lua that is GC work, amortised elsewhere.
- Top self time: `LjTab::find_node` / `newkey` / `rehashtab` (the Lua-table port; together about
  10 %, mostly output tables growing key by key, which Lua also rehashes), `CatBuf::intern`
  (3.4 %, `"Condition:" .. x` style names), `memcpy` / `malloc` / `free` (about 9 %),
  `ModStore::SumInternal` (1.8 %), `IndexMap::get_index_of` (1.5 %), `Ustr::from` (1.4 %),
  `ModStore::db_len` (1.4 %, already `#[inline]`; this is the hash lookup itself) and
  `__aarch64_ldadd8_rel` (0.5 %, `Arc<Mod>` reference counts). Nothing is above 3.6 % self.
- Cross-crate inlining after the split (calc-split-plan §3.2): the tiny accessors that show up as
  separate symbols (`db_len`, `list_named_len`, `Table::rawget`) are already `#[inline]`. What
  remains is their hash lookup, so adding `#[inline]` elsewhere would not help.
- Load (not gated): parsing tree stats (`PassiveTree::ProcessStats` → `parseMod` → Lua-pattern
  `find_bytes`) dominates `loadBuildFromXML`. Rust load is 0.6 s per build vs PoB's 0.3 s, but
  PoB keeps the processed tree across builds in one process and Rust builds it per load, so the
  two are not comparable.

Candidates for the owning units, if a later gate tightens: `CatBuf::intern` for constant
prefixes, pre-sizing large output tables (a Rust-side capacity hint; Lua's rehash sequence is
not observable), and the cost of `Arc<Mod>` reference counting in mod-list copies.

## 7. No semantic change (oracle gates, full corpus)

The calc's code is unchanged; INT-2 adds only a bench, a timing binary, a script and Cargo
entries. The gates were run on this branch with `POB_FULL_SUITE=1 POB_ORACLE_REQUIRE=1`, one
binary at a time through `cargo.sh`, pinned to CPUs 0-1:

- **CA** (961 s, 2,489 MB peak): `ca_l6_corpus` 57 builds, 0 PoB runs failed, 0 Rust runs
  raised. Headline 741/741 exact, all keys 45,954/45,954 exact, CALCS lists 171/171, used-tracking
  key sets 513/513, misc calculator 1,248/1,248. `ca_l5_post`: cost warnings 56/56 clean,
  used-tracking 55/55 clean, CALCS lists 57/57 clean (1 + 2 harness-limited, as before).
  `ca_d13_fixed_point`: 62 builds, 62 fixed points, 0 differ, 0 Lua errors.
- **PWR** (423 s, 1,612 MB): `pwr_l5_replay` 15 runs over 4 builds, **108,820/108,820** power
  values exact, 36,805 PoB calc calls replayed. `pwr_coroutine_steps` stepped vs direct exact.
  `pwr_l6_report` 7,672/7,672 exact; parallel vs sequential 7,672/7,672, 0 divergences.
  `pwr_stat_fns` ok.
- **AS** (37 s): `as_oracle` 5,015 cases, 1,942 exact, 3,066 LUA-ORDER, 7 AltX/AltY, **0
  MISMATCH**. `as_l4_corpus` 56/56. `as_census` and `as_copy_actor_keepalive` ok.

## 8. Caveats

- LuaJIT is the `mlua`-vendored 2.1, run in-process on aarch64 Linux. The Windows PoB release
  ships its own LuaJIT 2.1 build, which is not measured here. PoB's interpreter-level settings
  (`Launch.lua`) are applied.
- The full recalc is the calc part of a build change. `Build:OnFrame` also runs
  `skillsTab:UpdateSocketGroups()` and `RefreshStatList()`, which are UI and not part of either
  side.
- The machine was shared and loaded throughout. The minimum-CPU statistic and P-core pinning
  limit, but do not remove, that noise. Medians are shown for context.
