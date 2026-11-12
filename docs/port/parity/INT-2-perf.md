# INT-2 performance: power report and full recalc vs PoB under LuaJIT

Unit INT-2 (calc doc §11 row INT-2, §9.7 "Final (INT-2)"). First measured 2026-11-12 on
rust-port `fa733f3`, then re-measured the same day at low load on rust-port `91c97a3` (fix round 1
after review). The only calc change between the two is b13853e (one `levelCostCleared` lookup per
skill in the cost pass); the Rust times agree with the first version's within a few per cent.
Gate: the power report time ≤ PoB's LuaJIT time and the full recalc time ≤ PoB's LuaJIT time,
both single-threaded, release build, with the JIT on.

## 1. Result

**Gate met on every build measured, with no code change to the calc. On the light builds the
margin is small: allflame_exp_000's power report is about level with LuaJIT (0.95-0.99),
and the tightest recalcs are 0.92-0.97. Those margins are within run-to-run noise (§2 "Noise").**

All numbers are pinned to the performance cores, at load average about 2-4, with the order of the
two sides alternating per round (§2). Ratio = Rust min CPU / LuaJIT min CPU.

- Gate set (10 builds, 5 rounds, §3.1): power report **0.51-0.95** of LuaJIT's CPU time (total
  56.8 s vs 86.3 s, 0.66). Full recalc **0.70-0.85** (total 0.438 s vs 0.563 s, 0.78).
- **allflame_exp_000** (no minions, a PWR gate build) is the tight power case. Its power report
  is 0.95 here (Rust 4.43 s vs LuaJIT 4.65 s; per-round pairs 0.87-0.99). An independent review
  run measured 0.99 (LuaJIT 4.52-4.98 s, Rust 4.47-4.55 s). Rust is stable at 4.36-4.72 s across
  every session. LuaJIT ranges from 4.52 s to 5.57 s at a similar load average (§2 "Noise"). So
  the honest figure is "about level, Rust a few per cent ahead". The gate holds, but with no
  margin worth the name.
- The 4 other light builds with the highest power ratios (§3.2): 0.71-0.86.
- Full-corpus recalc survey (57 builds, 3 rounds x 20 recalcs, pinned, §4): Rust ≤ LuaJIT on 57/57,
  ratio 0.70-0.97 (median 0.84), total 1.710 s vs 2.064 s (0.83). The tightest builds are the light
  ones: allflame_exp_024 0.97, allflame_exp_021 0.97, allflame_exp_035 0.96, **allflame_exp_023
  0.92-0.96**, allflame_exp_000 0.85-0.96, allflame_exp_016 0.89-0.94, **allflame_exp_028
  0.88-0.96** (ranges over sessions, §3.2).
- Power, full corpus: only an unpinned survey exists (§4, ranking only). Its LuaJIT samples ran at
  load average 7-10 and are slowed. The pinned re-measurement covers the 16 builds of §3.
- No optimisation was applied in INT-2. The profile (§6) has no hot spot in INT-2's files
  (`calcs.rs`, `power.rs`), and no symbol above 3.6 % self time. The time is spread over the
  table, interning and mod-store primitives of other units. The gate is met, so INT-2 does not
  justify that work. But "≤ LuaJIT" holds on allflame_exp_000's power report and the light-build
  recalcs by only a few per cent. If a robust margin is wanted there, the §6 candidates are the
  place to start, in their owning units.
- Finding: an `Rc` cycle leaks memory. Each minion skill's `summonSkill` back-pointer closes a
  cycle, so every dropped env leaks its minion skills. One power report keeps up to 2.25 GB, and
  a recalc about 12 MB. This is **CCR-0135** (§5). The proposed fix gives bit-identical results
  on the gate set. Measured with it applied (§5), the gate set still passes: power 0.55-0.89,
  recalc 0.72-0.86. The fix does not touch allflame_exp_000 (no minion skill).
- No semantic change: CA, D13, PWR and AS are exact on the full corpus (§7).

## 2. Method

**What is timed.** The two sides run the same work.

| | PoB (`pob-oracle` bin `pob_calc_timing`) | Rust (`pob-calc` bench `INT-2`) |
|---|---|---|
| load | `main:SetMode("BUILD", …)` + one `OnFrame` (not gated) | `loadBuildFromXML` (not gated) |
| full recalc | `wipeGlobalCache()` + `calcsTab:BuildOutput()` (the calc part of `Build:OnFrame` on `buildFlag`, Build.lua:1258-1266: MAIN + CALCS `calcs.buildOutput`, which includes the cost-warning `buildActiveSkill` pass, and `getMiscCalculator`) | `common::wipeGlobalCache()` + `power::BuildOutput` |
| power report | `powerStat = nil` (Offence/Defence), `nodePowerMaxDepth = nil` (whole tree), `powerMax = nil`, then `calcsTab:PowerBuilder()` called directly (outside a coroutine it never yields; the work is the coroutine's) | the same settings, `power::PowerBuilder_run(…, PowerOptions::default())` (sequential, D12) |

Per process: load, 3 untimed recalcs, 10 timed recalcs (20 in the §3.2 and §4 runs), 1 timed
power report. Each round uses fresh processes (the default is 3 rounds; the §3.1 run used 5). The
two sides of a build run back to back, LuaJIT first in odd rounds and Rust first in even ones, so
neither side always gets the later slot. The summary takes the **minimum process CPU time**
(`CLOCK_PROCESS_CPUTIME_ID`) over all samples, which is the least disturbed run on a shared
machine. Wall time and medians are recorded too. Only one power report runs per process, because
a second one does not fit in 3 GB until CCR-0135 is applied (§5).

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
other agents. The first version of this report ran at load average 7-10. Every number in §3, §5
and the §4 recalc survey now comes from runs at load average about 2-4 (each run's `load.txt`
records the load at the start of every process).

**Pinning.** Both sides are pinned to the performance cores (`taskset -c 2-7`, the script's
default: the CPUs with the highest max frequency). The first, unpinned runs were dominated by
the scheduler moving a run onto an efficiency core. perf showed one Rust power report with about
20 % of its cycles on Icestorm, and the same binary on the same build varied by up to 1.6x
(allflame_exp_013 with the CCR-0135 patch: 9.1 s pinned, 13.2-14.5 s unpinned).

**Noise.** Pinning removes the efficiency-core effect, but not the other agents' load on the
shared caches and memory, and that load slows LuaJIT much more than Rust. On allflame_exp_000,
Rust's best power report is 4.36-4.47 s in every session. LuaJIT's best is 4.52-5.14 s at low
load and was 5.29 s in the first version at load 7-10 (§3.1). A plausible reason is the working
set: LuaJIT's process is about 1 GB against Rust's 0.5 GB on this build, and its GC walks that
heap. Whatever the cause, a ratio measured under load is optimistic for Rust. Even at the same
low load average, LuaJIT's best runs differ by up to 14 % between sessions (4.52 vs 5.14 s), and
the ratio of one back-to-back pair varies by 0.1 or more (the "pairs" columns). Ratios within
about 5 % of 1 are not a measurable lead.

**Reproduce.**

```sh
scripts/port/perf_bench.sh --rounds 5 --builds <§3.1 and §3.2 builds>      # §3.1, §3.2 first part
scripts/port/perf_bench.sh --rounds 8 --recalc 20 \
    --builds allflame_exp_000,allflame_exp_023,allflame_exp_028            # §3.2 second part
scripts/port/perf_bench.sh --builds all --power 0 --recalc 20 --rounds 3  # §4 corpus recalc
scripts/port/perf_bench.sh                       # the gate set (10 builds, 3 rounds)
scripts/port/perf_bench.sh --digest              # Rust only: output/power digests of every build
```

The whole measurement is one `cargo.sh bench -p pob-calc --bench INT-2`. cargo.sh compiles
under a compile slot and then runs `perf_bench.sh` itself as the bench's runner, under one run
slot. The script then starts each Rust bench process and each PoB `pob_calc_timing` process (the
latter takes the machine-wide oracle lock) one at a time, each pinned and in its own 3 GB scope.
The first version started one `cargo.sh bench` per Rust run. Behind long full-suite runs each of
those can wait minutes for a run slot (seen during this re-measurement), which moves the two
sides of a build apart in time. Outside `.claude/worktrees/*`, set `POB_PARITY_DIR`,
`POB_DUMP_DIR` and `POB_SRC` to the main checkout's `.parity`, `data/dump` and
`.PathOfBuilding/src`. The script finds `pob_calc_timing` wherever cargo.sh builds it, including
the `~/.cache/pob-port-targets/` dir of `/tmp` worktrees.

Results go to `target/int2-perf/<stamp>/{lua,rust}.jsonl`, `load.txt` and `summary.txt`. The
raw data of this version's four runs is kept in `docs/port/parity/INT-2-perf-data/`: `runA`
(§3.1 and the first part of §3.2), `runD` (the second part of §3.2), `runB` (§4 recalc) and
`runC-ccr0135` (§5, the calc patched with CCR-0135). The Rust
lines also carry an output digest and a power digest (exact f64 bits of every main/CALCS output
leaf and node power leaf), so a later optimisation can be checked bit for bit on the whole corpus
in minutes, before the oracle gates run.

## 3. Gate results (pinned, alternating, min CPU time)

### 3.1 Gate set

Build set (from the unpinned survey, §4): allflame_exp_013, 051, 030, 007, 039 and
upstream_Dual_Wield_Cospris_CoC are the 1st-4th, 7th and 9th heaviest builds by PoB power-report
time (8.6-13.3 s). allflame_exp_043 is the heaviest by PoB recalc time. allflame_exp_000 and
allflame_exp_011 had the 3rd- and 4th-highest Rust/LuaJIT power ratio in the survey (the higher
two and the next two are in §3.2); allflame_exp_000 is also a PWR gate build. allflame_exp_014 is
the PWR gate's cluster-jewel minion build.

Run 2026-11-12 08:30-08:54, 5 rounds (LuaJIT first in rounds 1, 3 and 5; Rust first in 2 and 4),
load average 1.7-3.8. The "pairs" column is the power ratio of each round's two back-to-back
processes. It shows the noise on one sample.

| build | recalc LuaJIT ms (min/med) | recalc Rust ms (min/med) | ratio | power LuaJIT s (min/med) | power Rust s (min/med) | ratio | power pairs | peak RSS MB LuaJIT / Rust |
|---|---|---|---|---|---|---|---|---|
| allflame_exp_013 | 47.6 / 52.2 | 39.3 / 40.8 | 0.82 | 12.41 / 13.27 | 8.47 / 8.66 | 0.68 | 0.64-0.69 | 886 / 1902 |
| allflame_exp_051 | 57.6 / 63.7 | 42.7 / 43.6 | 0.74 | 12.34 / 13.26 | 7.85 / 8.17 | 0.64 | 0.59-0.67 | 900 / 1616 |
| allflame_exp_030 | 70.2 / 80.9 | 49.1 / 50.7 | 0.70 | 10.13 / 10.78 | 5.13 / 5.24 | 0.51 | 0.45-0.51 | 803 / 1738 |
| allflame_exp_039 | 60.1 / 72.1 | 47.2 / 49.1 | 0.79 | 9.83 / 10.16 | 6.88 / 7.05 | 0.70 | 0.68-0.72 | 1004 / 2770 |
| upstream_Dual_Wield_Cospris_CoC | 53.5 / 57.9 | 41.8 / 44.0 | 0.78 | 8.99 / 9.16 | 5.75 / 5.92 | 0.64 | 0.64-0.68 | 874 / 1402 |
| allflame_exp_043 | 63.6 / 70.9 | 47.4 / 49.4 | 0.75 | 8.06 / 8.23 | 4.76 / 5.00 | 0.59 | 0.58-0.62 | 865 / 2132 |
| allflame_exp_007 | 59.8 / 70.9 | 47.3 / 48.6 | 0.79 | 7.44 / 7.80 | 4.86 / 5.01 | 0.65 | 0.61-0.66 | 848 / 2033 |
| allflame_exp_014 | 60.6 / 66.8 | 48.5 / 50.7 | 0.80 | 6.69 / 7.02 | 4.78 / 4.94 | 0.72 | 0.67-0.75 | 859 / 2045 |
| allflame_exp_011 | 50.4 / 55.6 | 41.1 / 42.4 | 0.81 | 5.81 / 5.85 | 3.91 / 4.06 | 0.67 | 0.64-0.72 | 801 / 1140 |
| **allflame_exp_000** | 39.4 / 52.7 | 33.6 / 35.1 | 0.85 | 4.65 / 4.83 | 4.43 / 4.58 | **0.95** | **0.87-0.99** | 1057 / 490 |

recalc: total min CPU LuaJIT 0.563 s, Rust 0.438 s, ratio 0.78. Power: LuaJIT 86.3 s, Rust
56.8 s, ratio 0.66. Every build ≤ LuaJIT on both. The Rust digests are the same in all rounds.
Peak RSS is the whole process (PoB includes its booted UI state). The Rust peaks above ~500 MB
are the CCR-0135 leak, and they flatter Rust's times on the minion builds (§5).

**allflame_exp_000 across sessions** (pinned, alternating, at load average about 2-4 in each):

| session | rounds | power LuaJIT s (min / all) | power Rust s (min / all) | ratio | recalc ratio |
|---|---|---|---|---|---|
| this report, gate run (above) | 5 | 4.65 / 4.65-5.09 | 4.43 / 4.43-4.72 | 0.95 | 0.85 |
| this report, close-build run (§3.2) | 8 | 5.14 / 5.14-5.57 (7.16 at load 7) | 4.36 / 4.36-4.72 (5.87 at load 4.5) | 0.85 | 0.93 |
| this report, corpus recalc run (§4) | 3 | - | - | - | 0.96 |
| independent review run | 7 | 4.52 / 4.52-4.98 | 4.47 / 4.47-4.55 | 0.99 | 0.82-0.89 |

Rust's best runs agree within 3 % in every session (4.36-4.47 s). LuaJIT's best runs differ by
14 % between sessions at the same load average (4.52-5.14 s). The 0.85 of the second session
comes from slower LuaJIT runs, not faster Rust ones. The figure to rely on is therefore the
fastest LuaJIT runs: **about 0.95-0.99, level within noise.** The first version of this report
gave 0.85 (LuaJIT 5.29 s, at load average 7-10). That number was the load, not the code.

### 3.2 Close builds

The light builds, where the ratios are highest. The first four had the highest power ratios in
the unpinned survey and were measured in the §3.1 run. allflame_exp_023 and allflame_exp_028 were
the tightest recalc builds in the review's pinned survey; they were measured together with
allflame_exp_000 in a second run (8 rounds x 20 recalcs, 2026-11-12 09:58-10:04, load average
1.8-2.7 except the last round, at 5-10).

| build | recalc LuaJIT ms (min/med) | recalc Rust ms (min/med) | ratio | power LuaJIT s (min/med) | power Rust s (min/med) | ratio | power pairs | peak RSS MB LuaJIT / Rust |
|---|---|---|---|---|---|---|---|---|
| allflame_exp_032 | 32.8 / 35.7 | 28.3 / 29.7 | 0.86 | 4.98 / 5.04 | 3.97 / 4.31 | 0.80 | 0.77-0.89 | 788 / 439 |
| allflame_exp_016 | 23.9 / 25.6 | 21.2 / 21.8 | 0.89 | 3.97 / 4.29 | 3.42 / 3.57 | 0.86 | 0.83-0.92 | 732 / 430 |
| allflame_exp_012 | 32.0 / 34.8 | 26.9 / 28.1 | 0.84 | 3.99 / 4.16 | 3.26 / 3.28 | 0.82 | 0.77-0.83 | 721 / 438 |
| upstream_OccVortex | 29.3 / 32.1 | 25.5 / 26.6 | 0.87 | 3.80 / 3.90 | 2.71 / 2.80 | 0.71 | 0.66-0.74 | 780 / 425 |
| **allflame_exp_023** | 12.8 / 14.2 | 11.8 / 12.2 | **0.92** | 2.73 / 2.87 | 2.07 / 2.09 | 0.76 | 0.72-0.87 | 728 / 428 |
| **allflame_exp_028** | 8.5 / 10.7 | 8.2 / 8.5 | **0.96** | 2.01 / 2.25 | 1.87 / 1.88 | 0.93 | 0.69-0.99 | 741 / 936 |
| allflame_exp_000 | 36.1 / 41.4 | 33.6 / 34.4 | 0.93 | 5.14 / 5.49 | 4.36 / 4.47 | 0.85 | 0.78-0.92 | 1065 / 490 |

Recalc ratios of the tight builds over all sessions: allflame_exp_023 0.92 (here), 0.96 (§4), 0.95
(review, 3 x 20 samples; 1.03 on one 5-sample process); allflame_exp_028 0.96 (here), 0.88 (§4),
0.93 (review); allflame_exp_000 0.85 (§3.1), 0.93 (here), 0.96 (§4), 0.82-0.89 (review);
allflame_exp_016 0.89 (§3.1), 0.94 (§4). These are **about 0.92-0.96: Rust ahead by a few per cent,
close to the noise.** One process of 5 recalcs is not enough to rank them: the first version of
this report put the highest corpus recalc ratio at 0.88, from such runs.

The margin is smallest on the light builds. On the heavy minion and mirage builds the Rust lead
is largest (0.51-0.72 power), partly because of the leak (§5).

## 4. Corpus surveys

- **Recalc, pinned** (2026-11-12 09:33-09:53, 3 rounds x 20 timed recalcs per side and build, one
  process per build and side, alternating, load average 1.3-4 for most of the run, peaks to 9):
  Rust ≤ LuaJIT on 57/57. Ratio of the minima 0.70-0.97 (median 0.84); ratio of the medians
  0.62-0.92 (median 0.78). Total 1.710 s vs 2.064 s (0.83). Highest ratios: allflame_exp_024 0.97
  (7.1 vs 6.9 ms), allflame_exp_021 0.97 (7.2 vs 6.9 ms), allflame_exp_035 0.96 (8.5 vs 8.2 ms),
  allflame_exp_023 0.96 (12.3 vs 11.8 ms), allflame_exp_000 0.96, allflame_exp_016 0.94,
  allflame_exp_010 0.93, allflame_exp_002 0.93. The smallest builds recalc in 7-8 ms on both
  sides, and LuaJIT's minimum is its luckiest of 60 runs (its median is 8-10 ms), so the min ratio
  is the conservative one.
- **Power, unpinned** (ranking only; from the first version of this report, load average 7-10),
  all 57 builds, 1 report each. LuaJIT ran in one batch process. Rust ran one process per build,
  because the batch process was OOM-killed at 5 GB after 13 builds (§5). Rust ≤ LuaJIT on 56/57,
  ratio 0.42-1.07 (median 0.64), total 248.8 s vs 408.3 s (0.61). The 1.07 is allflame_exp_012,
  from separate unpinned runs; pinned it is 0.82 (§3.2). The next highest ratios were
  upstream_OccVortex 0.93, allflame_exp_000 0.90, allflame_exp_011 0.84, allflame_exp_016 0.75 and
  allflame_exp_032 0.74. All were re-measured pinned (§3). Since the load slows LuaJIT more than
  Rust (§2 "Noise"), these ratios are optimistic for Rust; only the ranking is used.

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
applied here. Measured with the fix applied temporarily (pinned, alternating, 3 rounds,
2026-11-12 10:14-10:26, load average 1.5-3.3). The LuaJIT column is from that same run. The
unpatched Rust column is §3.1's (Rust's times agree within a few per cent across sessions, §2
"Noise").

| build | recalc ms LuaJIT / Rust / Rust+CCR-0135 | ratio Rust+CCR-0135 | power s LuaJIT / Rust / Rust+CCR-0135 | ratio Rust+CCR-0135 | RSS after power MB Rust / Rust+CCR-0135 | digests equal |
|---|---|---|---|---|---|---|
| allflame_exp_013 | 47.7 / 39.3 / 41.0 | 0.86 | 12.66 / 8.47 / 9.05 | 0.71 | 1902 / 484 | yes |
| allflame_exp_051 | 59.6 / 42.7 / 44.3 | 0.74 | 12.74 / 7.85 / 8.51 | 0.67 | 1616 / 485 | yes |
| allflame_exp_030 | 71.3 / 49.1 / 51.2 | 0.72 | 10.08 / 5.13 / 5.50 | 0.55 | 1738 / 445 | yes |
| allflame_exp_039 | 61.4 / 47.2 / 50.0 | 0.81 | 10.24 / 6.88 / 7.55 | 0.74 | 2770 / 691 | yes |
| upstream_Dual_Wield_Cospris_CoC | 52.5 / 41.8 / 43.4 | 0.83 | 9.03 / 5.75 / 6.13 | 0.68 | 1402 / 435 | yes |
| allflame_exp_043 | 61.9 / 47.4 / 49.8 | 0.80 | 7.73 / 4.76 / 5.02 | 0.65 | 2132 / 448 | yes |
| allflame_exp_007 | 62.0 / 47.3 / 49.7 | 0.80 | 7.74 / 4.86 / 5.16 | 0.67 | 2033 / 452 | yes |
| allflame_exp_014 | 62.7 / 48.5 / 51.2 | 0.82 | 6.90 / 4.78 / 5.47 | 0.79 | 2045 / 448 | yes |
| allflame_exp_011 | 51.4 / 41.1 / 42.2 | 0.82 | 5.82 / 3.91 / 4.07 | 0.70 | 1140 / 442 | yes |
| allflame_exp_000 | 39.5 / 33.6 / 33.5 | 0.85 | 4.95 / 4.43 / 4.39 | 0.89 | 490 / 490 | yes |

"Digests equal": the output and power digests of every round equal the unpatched calc's. The
fix costs time, because the leak saved the frees: power +4 %…+14 % (allflame_exp_014 4.78 →
5.47 s), recalc +3 %…+6 %, nothing on allflame_exp_000, which has no minion skill. With the fix
the gate set still passes: power 0.55-0.89 of LuaJIT (total 60.9 s vs 87.9 s, 0.69), recalc
0.72-0.86 (total 0.456 s vs 0.570 s, 0.80). The heavy builds keep a clear margin (power ≤ 0.79).
allflame_exp_000 is the same case as in §3.1: 0.89 by the minima, but one of its three round
pairs was 1.01, so it is level with LuaJIT within noise, with or without the fix. Re-run
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

Candidates for the owning units, if a robust margin is wanted on the light builds (§3:
allflame_exp_000's power report, the light-build recalcs): `CatBuf::intern` for constant
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
- The machine is shared. Low load, the minimum-CPU statistic, alternation and P-core pinning
  limit, but do not remove, the noise (§2 "Noise"). Ratios within about 5 % of 1
  (allflame_exp_000's power report, the light-build recalcs) are level within that noise, not a
  measured lead. Medians are shown for context.
