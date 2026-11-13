# Profiling suite of regular use (PROF1)

`pob-prof` replays what a user does in the app, on the real `pob_app::PobApp` with the Rust calc,
headless, and measures it: frame times, recalc latency, build load and start-up time, allocations
and peak RSS. Every repetition runs in a fresh process; the results are summarised as medians with
their dispersion, written as JSON, and compared against a committed baseline. Use it to hill-climb
on performance and to guard the idiomatic refactor against regressions.

It depends on no PoB code, no LuaJIT and no oracle crate (`cargo tree -p pob-prof | grep -c
"pob-oracle\|pob-ui-oracle\|mlua"` is 0): only on the product crates (pob-app, pob-host, pob-sg,
pob-sg-wgpu, pob-calc, pob-gamedata, pob-lua) and on the files the product itself reads.

Code: `crates/pob-prof` (library + the `pob-prof` binary), `scripts/perf/prof.sh`,
`scripts/perf/flamegraph.sh`. Scenarios: `crates/pob-prof/scenarios/*.pobscript`. Fixture user
directory: `crates/pob-prof/fixtures/user`. Baseline: `crates/pob-prof/baseline/baseline.json`.

## Quick start

```sh
scripts/perf/prof.sh list                        # the scenarios and their steps
scripts/perf/prof.sh run                         # everything: 5 measured reps + 1 warm-up, ~6 min
scripts/perf/prof.sh run --scenarios c_tree_alloc,e_items --reps 7 --out target/pob-prof/x.json
scripts/perf/prof.sh check                       # run everything, then compare with the baseline
scripts/perf/prof.sh compare crates/pob-prof/baseline/baseline.json target/pob-prof/x.json
scripts/perf/prof.sh inspect f_skills --png /tmp/f.png --texts   # one run in-process, a picture
scripts/perf/flamegraph.sh c_tree_alloc          # perf profile of the measured steps + hotspots
```

`prof.sh` builds `pob-prof` in the `prof` cargo profile (the release profile plus line tables; debug
info does not change code generation) through `scripts/port/cargo.sh` with `POB_PORT_JOBS=1`
(a release build of the whole app at 3 jobs can exceed cargo.sh's 5 GB scope), then runs it:
under one of cargo.sh's machine-wide run slots, in its own systemd scope (`MemoryMax`
`$POB_PERF_MEM`, default 3G; the heaviest scenario peaks at ~0.9 GB), pinned with `taskset` to
the fastest cores (`$POB_PERF_CPUS`; default every CPU with the highest max frequency: CPUs 2-7,
the Firestorm cores, on this M1 Pro; `none` disables pinning). `POB_PROF_NO_BUILD=1` skips the
build. Results go to `target/pob-prof/<UTC stamp>.json` (or `--out`) with a `.log` next to it
holding every child's stderr.

## Inputs

The app needs the compiled game data (`data.bin`, `trees/`) and its image and font assets. All of
it is resolved in one place, `crates/pob-prof/src/paths.rs`:

- **Assets**: `--assets DIR`, else `POB_PROF_ASSETS`, else `POB_ASSETS` (the override the VENDOR
  unit adds), else `POB_ROOT`, else the first `.PathOfBuilding/src` above the working directory or
  the executable. A directory with a `src/` subdirectory is read as PoB-shaped (`src/` +
  `runtime/SimpleGraphic/`); any other directory is used directly as the image root, with the fonts
  in its `runtime/SimpleGraphic`, `runtime` or itself (whichever holds `Fonts/`). When VENDOR's
  bundle lands with another layout, only `paths::locate` changes.
- **Game data**: `--data DIR`, else `POB_DUMP_DIR`, else `data.bin` in the assets directory, else the
  first `data/dump/data.bin` above the working directory or the executable.
- **User directory**: the scenario's `fixture userdir=../fixtures/user`: `Settings.xml` (opens in
  the build list) and 7 builds of the parity corpus, copied in (committed):

| fixture build | archetype | used by |
|---|---|---|
| allflame_exp_001 | spell (Ethereal Knives, Elementalist), cluster-heavy (27 cluster lines) | b, c, d, f, j |
| allflame_exp_005 | many items (40) and two item sets (Smite, Luminary) | b, e |
| allflame_exp_016 | DoT (Righteous Fire, Chieftain) | b, l |
| allflame_exp_032 | minion (Raise Spectre, Necromancer) | b, i, k |
| allflame_exp_048 | attack, small (Flicker Strike, Slayer) | i, k |
| upstream_Dual_Wield_Cospris_CoC | attack + trigger (Cyclone CoC, Raider), 3.13 tree | g |
| upstream_Generals_Perforate_Zerker | attack (Perforate, Berserker), 3.13 tree | b, h, j |

Files the app writes (saves, Settings.xml) go to pob-host's copy-on-write `HeadlessFs`: they stay
in memory, so runs never change the fixtures.

## Scenarios

Each scenario is a deterministic script over the fixture builds, at 1920x1080 (dpi 1). The steps
are what is measured; `_setup` (opening the build the scenario works on) is measured but never
gated.

| scenario | what the user does | steps |
|---|---|---|
| `a_startup` | app start to the build list | first_frames, background_load (until the item database loader is done), list_hover, list_idle |
| `b_open_build` | open builds: cold, warm, other archetypes and tree versions | open_cold, open_warm, open_attack_old_tree, open_minion, open_dot, open_many_items, idle_tree |
| `c_tree_alloc` | allocate/deallocate tree nodes one after another (each a recalc) | alloc_adjacent, dealloc_adjacent, alloc_path, alloc_keystone, dealloc_notable, undo_redo, dealloc_trunk |
| `d_tree_navigate` | pan, zoom, hover the tree (tooltips with stat diffs), search | idle, zoom_in, hover_nodes, sweep, zoom_out, zoomed_out_idle, drag, search |
| `e_items` | hover item slots, swap an item and back, change the item set, weapon swap | open_tab, hover_slots, swap_helmet, swap_helmet_back, item_set, item_set_back, weapon_swap |
| `f_skills` | gem level, gem toggle, add a support by typing (gem list sorted by DPS), remove it, change the main skill | open_tab, select_group, gem_level, gem_toggle, add_support_type, add_support_pick, remove_support, main_skill |
| `g_config` | hover options (tooltip recalcs), toggle player/enemy conditions, type a number, scroll | open_tab, hover_options, toggle_onslaught, toggle_enemy_shocked, type_power_charges, scroll |
| `h_calcs` | hover cells (breakdowns), pin a breakdown, scroll, switch the shown socket group | open_tab, hover_cells, pin_breakdown, scroll, switch_group |
| `i_power` | node power heat map and the power report, a recalc with the heat map on, a minion build's heat map | heat_map, power_report_list, level_change_rebuild, heat_map_minion |
| `j_compare` | Compare tab: import a second build by code, then every comparison view | open_tab, import_comparison, summary, tree_view, skills_view, items_view, calcs_view |
| `k_import` | Import/Export: generate a code, paste one, import it as a new build | open_tab, generate_code, paste_code, import_new_build |
| `l_save` | open from the list (double-click), edit, Ctrl+S, type a note, Save button | open_from_list, edit_level, save_ctrl_s, notes, save_button |
| `calc_<build>` (7) | the calc alone, per fixture build (`crates/pob-prof/src/stages.rs`) | full, initEnv, perform, buildOutput_MAIN, buildOutput_CALCS, getMiscCalculator, misc_call |

Every interaction carries guards (a click that must trigger a recalc, a hover that must compute a
tooltip's stat differences, a tab that must be open, a save that must write a file), so a layout
change that makes a click miss fails the run instead of silently measuring nothing.

## What is measured

Per step (`crates/pob-prof/src/driver.rs` `StepResult`; times in ms, memory in MB):

| field | meaning |
|---|---|
| `wall_ms` | elapsed time of the step, without its guards and snapshots |
| `cpu_ms` | the main thread's CPU time in the step (the app, the calc and the driver all run there); preemption by other load does not count, so it is the steadier number on a shared machine |
| `frames`, `frame_p50_ms`, `frame_p95_ms`, `frame_max_ms`, `frame_sum_ms` | frame time = `pob_host::run_frame` (`BeginFrame`, net completions, `launch:OnFrame`, `EndFrame`: sort and digest) + rendering |
| `input_ms` | input callbacks (`OnKeyDown`, `OnChar`, ...) and app steps (`load-build`) outside frames |
| `render_ms` | rendering (0 with `--render none`) |
| `tab_draw_ms` | the build tabs' `Draw` (Build.lua:1300-1318), inside the frames |
| `recalcs`, `recalc_ms`, `recalc_max_ms` | full recalcs (`calcsTab:BuildOutput`) seen through the timing `CalcFacade` (`calc_timing.rs`) |
| `misc_gets`, `misc_calls`, `misc_ms` | `GetMiscCalculator` and the misc calculator's calls (tooltip stat differences, gem and config previews) |
| `allocs`, `alloc_mb` | allocations (and reallocations) and bytes requested on the main thread (counting allocator, `alloc.rs`) |
| `heap_peak_mb` | peak live heap of the main thread during the step |
| `rss_peak_mb` | peak RSS during the step (VmHWM, reset at the step start through `/proc/self/clear_refs`) |
| `build_load_ms` | a build opened in the step: step start to the end of the first frame that shows it with its first recalc done |
| `power_ms` | `wait-power`: until the power report finished |

Per repetition: `startup.*` (`exec_ms`: the parent's spawn to `main`; `data_ms`: data.bin;
`assets_ms`: fonts; `app_new_ms`; `on_init_ms`: `launch:OnInit`; `first_frame_ms`;
`to_first_screen_ms`: `main` to the end of the first frame in LIST mode), `total.wall_ms` (sum of
the steps), `total.cpu_ms` (process CPU), `total.peak_rss_mb`.

**Recalc breakdown.** The app's recalc is timed as a whole (`recalc_ms`). The calc's stages are
timed from outside per fixture build (`calc_<build>`: `initEnv`, `perform` on its env,
`buildOutput(MAIN)` = initEnv + perform + calcFullDPS + the MAIN bookkeeping, `buildOutput(CALCS)`,
`getMiscCalculator`, one misc call). `offence` and `defence` run only inside `perform` and
`calcFullDPS` and cannot be timed without a hook in the calc (CCR-0150 proposes one); their share
comes from the sampling profile (`hotspots`, "inclusive share" lines).

**Render modes** (`--render`): `none` (default; the frame is built, sorted and digested by the app,
then dropped: the app's own CPU work), `cpu` (`pob_sg::raster`, the CPU reference rasteriser:
exact, not fast, ~200 ms per 1080p tree frame, so it is for pictures, not baselines), `wgpu` (the
shipping backend on an offscreen target, submitted and waited for: GPU work included; needs an
adapter, here Asahi's Vulkan driver). Baselines use `none`; record a `wgpu` run separately when
the renderer is the subject (`--render wgpu`; compare only runs of the same mode).

**Clocks.** `GetTime()` is the real clock (as on the desktop), so time-budgeted work behaves as a
user sees it: the power report yields after 100 ms of work (CalcsTab.lua:655) and spreads over
frames; the item database loader runs in the background over the first frames. Input timing
(double clicks) uses pob-host's virtual +16 ms/frame clock, so the scripted input means the same
in every run. `fixture clock=virtual` switches `GetTime()` to the virtual clock.

**Images** load synchronously (pob-host's headless loader), where the desktop decodes them on a
loader thread: the first frames after a tree or tab opens pay for image decodes in-frame.

## Reading results

`run` prints the medians (one line per step) and writes the results file (`pob-prof/1`,
`crates/pob-prof/src/results.rs`): `meta` (commit, dirty flag, date, render mode, reps, warm-up,
pinned CPUs, governor, CPU model, kernel, load average before and after), and per scenario its
steps, `metrics` (`<step>.<field>` -> median, MAD, relative MAD, min, max, n and the samples of the
measured repetitions) and `reps` (every repetition's raw `RepResult`, warm-up included). Failed
repetitions are listed in `errors` and make `run` exit 1. `pob-prof show FILE` prints the table
again.

The `±%` columns are the relative MAD over the repetitions: the noise of that number on that run.
Look at it before believing a difference.

## Baseline and comparison

`pob-prof compare BASE NEW` prints, for every metric in both files: the two medians and their
noise, the change of the median in percent, the two-sided Mann-Whitney p-value of the samples,
and a verdict: `REGRESSED` (a gated metric worse by more than its threshold with p < alpha),
`worse` / `better` (significant but under the threshold, or not gated), `~` (not significant).
Gated by default: the steps' `wall_ms`, `cpu_ms`, `frame_p95_ms`, `recalc_ms`, `build_load_ms`,
`power_ms`, `allocs`, `heap_peak_mb`, `rss_peak_mb`, and `startup.to_first_screen_ms`,
`total.wall_ms`, `total.cpu_ms`, `total.peak_rss_mb`; `_setup` steps never. Options:
`--threshold PCT` (default 10; the RSS fields default to 20, see Noise),
`--field-threshold allocs=2` (repeatable), `--alpha 0.05`, `--gate wall_ms,allocs` (replace the gated fields), `--all` (print
every metric), `--scenarios a,b`. Exit status: 1 when anything REGRESSED, 0 otherwise, 2 on usage
errors. The last lines give the geometric mean of the `wall_ms` and `cpu_ms` ratios (new/base) over
every gated step: the single number to hill-climb on.

**Bless a baseline**: run the whole suite on an idle machine (§Noise), check `errors` is empty
and the medians look sane, then `scripts/perf/prof.sh bless target/pob-prof/<run>.json`. It writes
`crates/pob-prof/baseline/baseline.json` without the raw repetitions (the summaries keep every
sample) and refuses a run with failed repetitions. Commit it with a line in this file's "Baseline"
section (date, commit, load). Re-bless after an intended performance change, never to hide one.

## Hill-climb loop

```sh
scripts/perf/prof.sh run --scenarios c_tree_alloc --reps 9 --out target/pob-prof/before.json
scripts/perf/flamegraph.sh c_tree_alloc --steps alloc_adjacent,alloc_path   # where the time goes
# ... change one thing ...
scripts/perf/prof.sh run --scenarios c_tree_alloc --reps 9 --out target/pob-prof/after.json
scripts/perf/prof.sh compare target/pob-prof/before.json target/pob-prof/after.json
# the oracle-free correctness gates still apply to every optimisation
```

Measure before and after in the same session (same load, same build of everything else), use
more repetitions for small effects (the noise floor below), prefer `cpu_ms` on a busy machine,
and keep the `allocs` column in view: allocation counts are deterministic, so they move
measurably even when time is lost in the noise. Do not optimise inside the measurement harness.

## Profiling hooks

**Flamegraph of a scenario**: `scripts/perf/flamegraph.sh <scenario> [--steps a,b] [--freq 999]
[--render none|wgpu] [--out DIR]`. It records `perf record -g` (frame pointers; the aarch64 build
keeps them) with sampling enabled only during the measured steps (a perf control fifo that
`pob-prof` drives: `POB_PROF_PERF_CTL`, `POB_PROF_PERF_STEPS`; start-up and `_setup` are not in
the profile), folds the stacks (`pob-prof collapse`, no external tool needed), prints the hotspot
summary (`pob-prof hotspots`: top functions by self time, self time by crate, inclusive share of
the calc stages and app areas) and writes `DIR/flame.svg` when `inferno-flamegraph` (`cargo
install inferno`) or `flamegraph.pl` is on PATH; otherwise feed `DIR/folded.txt` to any
flamegraph tool or to speedscope. Needs `perf_event_paranoid <= 2`.

**Heap profile**: `scripts/port/cargo.sh build --profile prof -p pob-prof --features dhat-heap`,
then `target/prof/pob-prof exec <scenario>` writes `dhat-heap.json` (view it with dhat's
`dh_view.html`). The dhat build replaces the counting allocator (its `allocs` read 0).

**One run, in-process**: `pob-prof inspect <scenario> [--until STEP] [--png FILE] [--texts]
[--render MODE]` prints the step table; `--png` keeps every texture and writes the last frame
through the CPU rasteriser; `--texts` prints every string drawn in the last frame of each step with
its position (how coordinates for new scenario steps are found); `--until` stops after a step.

**Calc stages of any build**: `pob-prof stages <build.xml>... [--iters 5]`.

## Adding a scenario

1. Copy a scenario in `crates/pob-prof/scenarios/` (the file name is its name; the first comment
   line its description). Lines are pob-host's `.pobscript` DSL (`crates/pob-host/src/script.rs`:
   `screen`, `fixture`, `load-build`, `frames`, `move`, `click`, `key-down`, `key-up`, `press`,
   `type`, `wheel`, `paste-clipboard`, `snapshot`, `exit`) plus the suite's directives
   (`crates/pob-prof/src/scenario.rs`): `step NAME`, `wait-idle [MAX]`, `wait-power [MAX]`,
   `clipboard-build-code "Builds/x.xml"`, and the guards `expect-recalc [N]`,
   `expect-no-recalc`, `expect-misc [N]`, `expect-file-write [N]`, `expect-mode LIST|BUILD`,
   `expect-tab TREE|SKILLS|ITEMS|CALCS|CONFIG|IMPORT|NOTES|PARTY|COMPARE`, `expect-power`,
   `expect-differs A B`, `expect-same A B`, `expect-calc-func`.
2. Open the build in the set-up and end the set-up with `wait-idle`, so the measured steps do not
   include the background loading that follows every build open (measure that on purpose, as
   `b_open_build` does).
3. Find coordinates with `pob-prof inspect <your scenario> --until STEP --png /tmp/x.png --texts`
   (look at the picture; the strings list gives exact label positions, and a hovered tree node's
   tooltip names the node). Put a guard after every interaction.
4. `pob-prof inspect <scenario>` until it passes, then `cargo.sh test -p pob-prof` (every committed
   scenario must parse and have a description and a step), then bless a new baseline (a new
   scenario is reported as "new scenario (not in BASE)" until then).

## Noise

Measured 2026-11-13 on this machine (Apple M1 Pro, 8 cores: 0-1 Icestorm 2.06 GHz, 2-7
Firestorm 3.04 GHz; Asahi Linux 7.1; 15 GB shared with ~10 other agents), commit `0dbdab33` + this
unit, `--render none`, pinned to CPUs 2-7, governor `schedutil` (not pinned), 5 measured reps + 1
warm-up per scenario, load average 2.4-4.8 during the runs. Two full runs back to back (A, then B:
an A/A test, same binary), 121 measured steps each:

| field | within-run relative MAD (A): median / p90 / max | A vs B, \|change of the median\|: median / p90 / max |
|---|---|---|
| `wall_ms` | 0.32 % / 0.86 % / 6.5 % | 0.63 % / 1.9 % / 5.5 % |
| `cpu_ms` | 0.29 % / 0.78 % / 6.4 % | 0.69 % / 2.0 % / 5.8 % |
| `frame_p95_ms` | 0.40 % / 1.3 % / 3.5 % | 0.68 % / 2.9 % / 7.6 % |
| `recalc_ms` | 0.22 % / 0.58 % / 1.0 % | 0.35 % / 2.4 % / 3.7 % |
| `build_load_ms` | 0.51 % / 1.4 % / 1.8 % | 1.2 % / 1.7 % / 2.9 % |
| `power_ms` | 0.20 % / 0.36 % / 0.36 % | 2.6 % / 5.8 % / 5.8 % |
| `allocs` | 0 % (deterministic but for 2 steps) / 0 % / 5.0 % | 0 % / 0 % / 1.7 % |
| `heap_peak_mb` | 0 % | 0 % (deterministic) |
| `rss_peak_mb` | 0.16 % / 0.39 % / 2.8 % | 0.32 % / **13.5 %** / 16 % (bimodal per process: the post-reset resident size lands at ~398 or ~452 MB) |
| `startup.to_first_screen_ms` | 0.5 % | 1086-1095 ms over the 12 scenarios |

`compare A B` (the default gates, threshold 10 %): **0 of 732 gated metrics REGRESSED**; the
geometric mean of the step ratios is 1.0058 (wall) and 1.0060 (cpu). So:

- **Noise floor**: a single step moves by up to ~2 % (p90) and rarely ~6 % between two runs of the
  same binary in one session; the suite-wide geomean by ~0.6 %. A change below ~2 % on one step,
  or below ~1 % on the geomean, is not distinguishable from noise with 5 reps; use more reps
  (`--reps 9` or more) and same-session before/after runs for small effects. The p-value catches
  consistent shifts smaller than the threshold (3 of 732 metrics were `better` at p < 0.05 in the
  A/A test: expect a few such false hints, which is why only `> threshold` and `p < alpha` fails).
- `wall_ms` and `cpu_ms` agree within ~0.5 % here (pinned, 6 cores for one busy thread); under
  heavier load `cpu_ms` is the one to trust. Allocation counts and heap peaks are exact and are
  the cheapest signal of a change.
- RSS peaks are bimodal between processes; their gate is 20 % (`field_threshold`), the others
  10 %.
- **Counting allocator overhead**: a third run with counting off (`POB_PROF_ALLOC_COUNT=0`, 72
  steps) had a geomean `cpu_ms` ratio of 0.9989 against A (0.9997 over the steps longer than 50
  ms): the counting costs about 0.1 %, inside the noise. (The first version used shared atomics:
  1.4 % of the samples of a recalc-heavy scenario were the aarch64 out-of-line atomic, so the
  counters became thread-local.)

**Making runs steadier** (all optional; record what you did in the run's `--label`):

- Pin the governor (needs root): `echo performance | sudo tee
  /sys/devices/system/cpu/cpu{2..7}/cpufreq/scaling_governor` (back with `schedutil`). The run
  records the governor, and `run` warns when it is not `performance`. Not done for the baseline.
- Keep the machine quiet: the results record the load average before and after (`meta.load_*`);
  the baseline ran at 2.4-4.8 with other agents compiling.
- Pin to one core (`POB_PERF_CPUS=7`) to remove migrations, at the risk of sharing that core with
  another process; the default pins to the six fast cores.
- More repetitions; `taskset`/`systemd-run` are already applied by `prof.sh`.

## Baseline

`crates/pob-prof/baseline/baseline.json`: run A of the noise test above (2026-11-13T09:05Z,
commit `0dbdab33` + PROF1, `--render none`, CPUs 2-7, governor schedutil, load 2.6 -> 3.4, 5 reps
+ 1 warm-up, 350 s). Medians; `±%` is the relative MAD; `load ms` is `build_load_ms`. Full
numbers: `pob-prof show crates/pob-prof/baseline/baseline.json`.

Start-up (median of all 12 app scenarios): `main` to the build list on screen **1.09 s** =
data.bin 382 ms + fonts 66 ms + `launch:OnInit` 583 ms + the first frame 57 ms; `exec` 0.9 ms.
After that the build list keeps loading the item database in the background for ~0.5 s
(`a_startup.background_load`, 14 frames of 28-50 ms). Peak RSS 550-585 MB per scenario process
(`i_power` the highest); the calc-only processes peak at 382 MB.

| scenario | step | wall ms (±%) | cpu ms | frames | frame p50 / p95 ms | recalcs (ms) | misc calls | allocs | load ms |
|---|---|---|---|---|---|---|---|---|---|
| a_startup | first_frames | 126.8 (0.5) | 126.3 | 3 | 34.71 / 56.70 | 0 (0.0) | 0 | 377,630 | - |
| a_startup | background_load | 509.4 (0.1) | 507.7 | 14 | 27.90 / 49.79 | 0 (0.0) | 0 | 1,654,313 | - |
| a_startup | list_hover | 0.2 (0.7) | 0.2 | 9 | 0.03 / 0.03 | 0 (0.0) | 0 | 1,617 | - |
| a_startup | list_idle | 0.8 (0.2) | 0.8 | 30 | 0.03 / 0.03 | 0 (0.0) | 0 | 5,375 | - |
| b_open_build | _setup | 635.0 (0.1) | 633.2 | 17 | 33.72 / 56.76 | 0 (0.0) | 0 | 2,031,942 | - |
| b_open_build | open_cold | 569.1 (1.1) | 567.2 | 5 | 2.44 / 559.12 | 1 (34.6) | 0 | 1,048,162 | 559.2 |
| b_open_build | open_warm | 110.4 (0.2) | 110.0 | 5 | 2.50 / 99.51 | 1 (30.6) | 0 | 766,203 | 99.6 |
| b_open_build | open_attack_old_tree | 670.7 (0.5) | 668.7 | 5 | 1.86 / 662.42 | 1 (29.4) | 0 | 1,739,265 | 662.5 |
| b_open_build | open_minion | 118.9 (0.2) | 118.6 | 5 | 2.48 / 108.23 | 1 (27.1) | 0 | 749,734 | 108.3 |
| b_open_build | open_dot | 115.0 (0.0) | 114.6 | 5 | 2.56 / 103.98 | 1 (19.7) | 0 | 675,293 | 104.0 |
| b_open_build | open_many_items | 154.7 (0.4) | 154.2 | 5 | 2.52 / 143.75 | 1 (36.5) | 0 | 860,058 | 143.8 |
| b_open_build | idle_tree | 49.5 (1.2) | 49.3 | 20 | 2.47 / 2.50 | 0 (0.0) | 0 | 275,065 | - |
| c_tree_alloc | _setup | 1,233.7 (0.5) | 1,229.9 | 20 | 30.74 / 122.00 | 1 (34.0) | 0 | 3,258,684 | 680.4 |
| c_tree_alloc | alloc_adjacent | 61.6 (1.5) | 61.3 | 7 | 2.61 / 39.23 | 1 (31.4) | 2 | 368,368 | - |
| c_tree_alloc | dealloc_adjacent | 53.2 (0.6) | 52.9 | 5 | 2.58 / 38.97 | 1 (31.4) | 1 | 328,576 | - |
| c_tree_alloc | alloc_path | 63.5 (0.3) | 63.2 | 7 | 2.66 / 39.32 | 1 (31.6) | 3 | 379,741 | - |
| c_tree_alloc | alloc_keystone | 61.5 (0.3) | 61.2 | 7 | 2.60 / 39.27 | 1 (31.6) | 2 | 369,310 | - |
| c_tree_alloc | dealloc_notable | 61.6 (0.1) | 61.4 | 7 | 2.66 / 39.46 | 1 (31.6) | 2 | 368,964 | - |
| c_tree_alloc | undo_redo | 156.0 (0.2) | 155.5 | 10 | 11.12 / 37.41 | 3 (95.3) | 0 | 993,480 | - |
| c_tree_alloc | dealloc_trunk | 102.3 (1.2) | 101.9 | 10 | 5.08 / 34.57 | 2 (48.7) | 5 | 580,828 | - |
| d_tree_navigate | _setup | 1,219.7 (0.5) | 1,216.0 | 17 | 33.90 / 121.68 | 1 (34.2) | 0 | 3,215,912 | 672.5 |
| d_tree_navigate | idle | 50.7 (1.5) | 50.5 | 20 | 2.52 / 2.62 | 0 (0.0) | 0 | 277,226 | - |
| d_tree_navigate | zoom_in | 15.4 (1.2) | 15.3 | 6 | 2.58 / 2.66 | 0 (0.0) | 0 | 84,113 | - |
| d_tree_navigate | hover_nodes | 59.6 (0.6) | 59.3 | 15 | 2.80 / 7.03 | 0 (0.0) | 7 | 301,234 | - |
| d_tree_navigate | sweep | 70.6 (0.2) | 70.3 | 20 | 2.59 / 7.32 | 0 (0.0) | 8 | 371,015 | - |
| d_tree_navigate | zoom_out | 27.6 (0.9) | 27.5 | 9 | 2.57 / 7.09 | 0 (0.0) | 2 | 149,224 | - |
| d_tree_navigate | zoomed_out_idle | 25.3 (0.5) | 25.1 | 10 | 2.52 / 2.56 | 0 (0.0) | 0 | 140,534 | - |
| d_tree_navigate | drag | 20.4 (0.4) | 20.3 | 8 | 2.53 / 2.60 | 0 (0.0) | 0 | 111,196 | - |
| d_tree_navigate | search | 31.3 (0.4) | 31.2 | 8 | 2.70 / 12.69 | 0 (0.0) | 0 | 204,933 | - |
| e_items | _setup | 1,245.1 (0.3) | 1,241.1 | 17 | 33.98 / 121.85 | 1 (39.2) | 0 | 3,306,612 | 696.2 |
| e_items | open_tab | 8.4 (0.4) | 8.3 | 5 | 0.86 / 4.52 | 0 (0.0) | 0 | 69,433 | - |
| e_items | hover_slots | 29.1 (0.8) | 29.0 | 10 | 3.50 / 6.36 | 0 (0.0) | 5 | 134,571 | - |
| e_items | swap_helmet | 64.6 (0.3) | 64.4 | 13 | 0.94 / 5.42 | 1 (36.6) | 3 | 432,629 | - |
| e_items | swap_helmet_back | 65.9 (0.2) | 65.6 | 12 | 0.94 / 5.38 | 1 (36.8) | 4 | 448,109 | - |
| e_items | item_set | 52.2 (0.4) | 52.0 | 12 | 0.88 / 2.87 | 1 (33.8) | 0 | 350,609 | - |
| e_items | item_set_back | 55.0 (0.1) | 54.9 | 12 | 0.90 / 2.82 | 1 (36.8) | 0 | 385,955 | - |
| e_items | weapon_swap | 86.7 (0.2) | 86.4 | 12 | 0.99 / 33.63 | 2 (64.2) | 0 | 642,064 | - |
| f_skills | _setup | 1,223.6 (0.1) | 1,218.8 | 17 | 33.99 / 121.62 | 1 (34.1) | 0 | 3,215,912 | 675.2 |
| f_skills | open_tab | 1.0 (0.6) | 1.0 | 5 | 0.17 / 0.24 | 0 (0.0) | 0 | 6,882 | - |
| f_skills | select_group | 3.5 (0.8) | 3.5 | 5 | 0.63 / 1.39 | 0 (0.0) | 0 | 33,059 | - |
| f_skills | gem_level | 76.4 (0.1) | 76.2 | 10 | 1.09 / 34.25 | 2 (61.7) | 0 | 566,500 | - |
| f_skills | gem_toggle | 83.8 (0.2) | 83.5 | 10 | 1.29 / 36.10 | 2 (61.8) | 3 | 612,134 | - |
| f_skills | add_support_type | 314.4 (0.1) | 313.5 | 7 | 51.23 / 57.40 | 0 (0.0) | 152 | 1,774,397 | - |
| f_skills | add_support_pick | 205.1 (0.8) | 204.4 | 4 | 51.56 / 56.98 | 1 (31.5) | 82 | 1,298,251 | - |
| f_skills | remove_support | 99.6 (6.5) | 99.2 | 6 | 6.10 / 51.05 | 1 (31.4) | 28 | 650,628 | - |
| f_skills | main_skill | 117.8 (0.5) | 117.5 | 24 | 1.96 / 36.01 | 2 (63.6) | 0 | 999,318 | - |
| g_config | _setup | 1,752.0 (0.6) | 1,747.4 | 17 | 33.79 / 120.65 | 1 (39.2) | 0 | 4,124,630 | 1,216.9 |
| g_config | open_tab | 26.8 (0.2) | 26.8 | 5 | 4.55 / 8.07 | 0 (0.0) | 0 | 152,596 | - |
| g_config | hover_options | 67.4 (0.1) | 67.1 | 10 | 8.56 / 9.16 | 0 (0.0) | 5 | 389,085 | - |
| g_config | toggle_onslaught | 150.9 (0.2) | 150.4 | 10 | 5.55 / 50.77 | 2 (78.3) | 4 | 963,850 | - |
| g_config | toggle_enemy_shocked | 158.5 (0.7) | 158.0 | 10 | 8.65 / 51.43 | 2 (79.1) | 5 | 987,480 | - |
| g_config | type_power_charges | 151.7 (0.2) | 151.1 | 13 | 4.85 / 47.48 | 2 (79.4) | 0 | 966,944 | - |
| g_config | scroll | 28.7 (0.3) | 28.6 | 4 | 9.76 / 9.82 | 0 (0.0) | 0 | 150,672 | - |
| h_calcs | _setup | 1,747.0 (0.2) | 1,741.7 | 17 | 34.19 / 121.05 | 1 (29.4) | 0 | 4,157,917 | 1,208.4 |
| h_calcs | open_tab | 5.9 (0.6) | 5.9 | 5 | 1.08 / 1.53 | 0 (0.0) | 0 | 41,028 | - |
| h_calcs | hover_cells | 14.1 (0.8) | 14.0 | 12 | 1.16 / 1.31 | 0 (0.0) | 0 | 103,290 | - |
| h_calcs | pin_breakdown | 12.2 (0.7) | 12.1 | 11 | 1.10 / 1.13 | 0 (0.0) | 0 | 86,039 | - |
| h_calcs | scroll | 6.8 (0.3) | 6.8 | 6 | 1.15 / 1.25 | 0 (0.0) | 0 | 48,959 | - |
| h_calcs | switch_group | 89.2 (0.6) | 88.9 | 24 | 1.12 / 31.96 | 2 (58.0) | 0 | 618,755 | - |
| i_power | _setup | 1,205.5 (0.3) | 1,202.5 | 17 | 33.83 / 121.28 | 1 (28.8) | 0 | 3,163,119 | 663.5 |
| i_power | heat_map | 4,075.1 (0.2) | 4,057.8 | 42 | 104.86 / 105.97 | 0 (0.0) | 0 | 23,085,981 | - |
| i_power | power_report_list | 29.1 (0.2) | 29.0 | 9 | 3.19 / 3.33 | 0 (0.0) | 0 | 98,500 | - |
| i_power | level_change_rebuild | 4,156.2 (0.3) | 4,137.9 | 43 | 104.91 / 106.23 | 1 (28.5) | 0 | 23,268,352 | - |
| i_power | heat_map_minion | 4,498.2 (0.2) | 4,472.2 | 49 | 105.62 / 106.60 | 1 (26.8) | 0 | 26,837,293 | 116.4 |
| j_compare | _setup | 1,224.8 (0.8) | 1,221.0 | 17 | 33.81 / 121.35 | 1 (34.0) | 0 | 3,215,912 | 679.5 |
| j_compare | open_tab | 5.5 (0.3) | 5.5 | 7 | 0.11 / 2.51 | 0 (0.0) | 0 | 32,092 | - |
| j_compare | import_comparison | 608.3 (0.1) | 606.7 | 14 | 0.37 / 1.06 | 1 (30.6) | 0 | 1,305,474 | - |
| j_compare | summary | 3.6 (0.0) | 3.6 | 6 | 0.57 / 0.68 | 0 (0.0) | 0 | 34,351 | - |
| j_compare | tree_view | 63.0 (0.2) | 62.8 | 10 | 3.22 / 40.85 | 0 (0.0) | 0 | 405,690 | - |
| j_compare | skills_view | 7.4 (1.4) | 7.4 | 6 | 0.43 / 3.08 | 0 (0.0) | 0 | 47,377 | - |
| j_compare | items_view | 17.3 (0.1) | 17.2 | 8 | 0.37 / 8.11 | 0 (0.0) | 4 | 117,424 | - |
| j_compare | calcs_view | 16.6 (0.1) | 16.6 | 8 | 2.42 / 3.70 | 0 (0.0) | 0 | 144,177 | - |
| k_import | _setup | 1,215.6 (0.7) | 1,212.0 | 17 | 33.98 / 121.77 | 1 (28.8) | 0 | 3,163,119 | 666.9 |
| k_import | open_tab | 5.5 (0.9) | 5.5 | 7 | 0.15 / 2.45 | 0 (0.0) | 0 | 32,688 | - |
| k_import | generate_code | 11.6 (0.1) | 11.6 | 5 | 0.30 / 10.68 | 0 (0.0) | 0 | 52,225 | - |
| k_import | paste_code | 3.9 (0.5) | 3.9 | 6 | 0.51 / 1.91 | 0 (0.0) | 0 | 7,373 | - |
| k_import | import_new_build | 123.3 (1.4) | 122.8 | 9 | 2.58 / 105.73 | 1 (27.0) | 0 | 768,423 | 106.7 |
| l_save | _setup | 635.9 (0.6) | 633.8 | 17 | 33.12 / 56.89 | 0 (0.0) | 0 | 2,031,942 | - |
| l_save | open_from_list | 539.5 (2.0) | 538.2 | 11 | 2.49 / 526.89 | 1 (21.6) | 0 | 970,483 | 527.1 |
| l_save | edit_level | 33.9 (0.7) | 33.7 | 5 | 2.51 / 23.83 | 1 (19.7) | 0 | 215,309 | - |
| l_save | save_ctrl_s | 21.7 (0.7) | 21.7 | 3 | 2.45 / 16.94 | 0 (0.0) | 0 | 103,653 | - |
| l_save | notes | 1.0 (0.8) | 1.0 | 9 | 0.11 / 0.15 | 0 (0.0) | 0 | 7,893 | - |
| l_save | save_button | 14.4 (0.4) | 14.4 | 5 | 0.10 / 13.98 | 0 (0.0) | 0 | 66,294 | - |

Calc stages per fixture build (`calc_<build>`, main-thread CPU ms; `full` is the app's recalc,
`buildOutput_MAIN` includes `initEnv` + `perform` + `calcFullDPS` + the cost-warning
`buildActiveSkill` passes):

| calc stages (ms, median of 5 iterations, median over reps) | full | initEnv | perform | buildOutput_MAIN | buildOutput_CALCS | getMiscCalculator | misc_call |
|---|---|---|---|---|---|---|---|
| allflame_exp_001 | 32.76 | 1.16 | 1.91 | 25.36 | 3.08 | 2.42 | 1.87 |
| allflame_exp_005 | 38.60 | 1.45 | 1.99 | 29.74 | 3.53 | 2.88 | 2.17 |
| allflame_exp_016 | 20.84 | 1.06 | 1.44 | 14.35 | 2.77 | 2.24 | 1.64 |
| allflame_exp_032 | 27.37 | 1.35 | 1.48 | 19.83 | 2.92 | 2.51 | 1.85 |
| allflame_exp_048 | 27.50 | 1.19 | 1.71 | 20.20 | 3.10 | 2.48 | 1.84 |
| upstream_Dual_Wield_Cospris_CoC | 41.06 | 1.31 | 3.52 | 30.24 | 4.78 | 4.04 | 3.53 |
| upstream_Generals_Perforate_Zerker | 30.54 | 1.05 | 3.07 | 22.74 | 2.57 | 3.47 | 2.96 |

A `wgpu` run of `d_tree_navigate` (Asahi Vulkan, offscreen 1080p, GPU waited for) gave tree frames
of ~8.5 ms p50 against 2.5 ms with `--render none`; the CPU rasteriser takes ~200 ms per frame.
No `wgpu` baseline is committed (record one with `--render wgpu` when working on the renderer).

## Hotspots (starting list for the performance work)

From `scripts/perf/flamegraph.sh <scenario>` on the baseline commit (999 Hz, frame pointers,
only the measured steps sampled; `--top 5`). Self time is the leaf frame; `[libc.so.6]` is libc
code without symbols (mostly `memset`/`memcpy`/allocator internals). Inclusive shares are from the
same profiles. These are the starting list for the performance work; nothing was optimised here.

| scenario (samples) | top 5 functions by self time | where the time goes (inclusive) |
|---|---|---|
| a_startup (619) | `pob_lua::pattern::do_match` 49.3 %, `pattern::classend` 7.9 %, `pattern::matchbracketclass` 7.1 %, `pattern::gsub_bytes_with` 6.9 %, `memset` 5.7 % | Lua-pattern string work is 76 % of the samples: the background item database load, `Item::ParseRaw` (40 % via `new_Item`), `item_tools::applyRange` (9 %), `escapeGGGString` (6 %) |
| b_open_build (1761) | `pattern::do_match` 19.1 %, `memset` 14.0 %, `pattern::find_bytes` 10.2 %, `memcmp` 5.7 %, `[libc]` 4.0 % | build load 88.5 % (`Build:Init`); the mod parser's `scan` / `parseModPassWith` from `mod_cache::parseMod` is ~45 % of all samples; recalc 10.2 %, tree draw 9.9 %, image decode 2.4 % |
| c_tree_alloc (542) | `LjTab::newkey` 8.3 %, `[libc]` 7.7 %, `LjTab::find_node` 4.1 %, `PassiveSpec::BuildAllDependsAndPaths` 3.5 %, `malloc` 3.5 % | recalc 54.8 % (initEnv 28 %, perform 31 %, offence 12.7 %, defence 4.6 %, defence estimations 3.1 %, calcFullDPS 3.7 %); tree view draw 31 %, tooltips 7 %, allocator 9 % |
| d_tree_navigate (286) | `IndexMap<Key, Val>::get` 10.5 %, `memcpy` 7.3 %, `pob_sg::frame::digest` 7.3 %, `drop_glue::<Val>` 7.0 %, `PassiveTreeView::draw_nodes` 6.6 % | tree view draw 86.7 %; tooltips 13.6 % (misc calculator 12.9 %); the frame digest + sort 9.4 % |
| e_items (349) | `[libc]` 7.2 %, `LjTab::find_node` 5.2 %, `LjTab::newkey` 4.0 %, `malloc` 3.7 %, `cfree` 3.2 % | recalc 59.9 % (initEnv 32 %, perform 31 %, offence 9.5 %); tooltips 8.3 %; allocator 10 % |
| f_skills (879) | `[libc]` 8.9 %, `LjTab::newkey` 7.7 %, `LjTab::find_node` 6.9 %, `memcpy` 4.2 %, `malloc` 3.9 % | misc calculator 57.5 % (the gem list sorted by DPS: 152 calls while typing three letters, ~2 ms each); perform 49.7 %, initEnv 31.9 %, offence 19.2 % |
| g_config (569) | `Cx::is_child_of` 8.3 %, `[libc]` 7.4 %, `LjTab::newkey` 4.4 %, `LjTab::find_node` 3.9 %, `cfree` 3.9 % | recalc 41 %; triggers/mirages 10.7 % (CoC); tooltips 10 %; `pob_ui_core` 10.4 % self (the control tree walk: Config frames cost 8.6 ms p50 at idle) |
| h_calcs (113) | `pattern::do_match` 14.2 %, `LjTab::newkey` 6.2 %, `[libc]` 4.4 %, `memcpy` 4.4 %, `Table::rawget` 3.5 % | recalc 52 % (the group switch); Calcs frames themselves are cheap (1.1 ms) |
| i_power (12758) | `LjTab::newkey` 9.6 %, `LjTab::find_node` 8.3 %, `[libc]` 6.5 %, `Table::rawget` 4.8 %, `memcpy` 3.7 % | power report 94.9 % = misc calculator calls 94.3 % (perform 54.9 %, initEnv 39.4 %, defence estimations 17.5 %, offence 14.1 %); 4.1-4.5 s per report |
| j_compare (713) | `pattern::do_match` 26.4 %, `memset` 17.1 %, `pattern::find_bytes` 12.6 %, `pattern::classend` 5.3 %, `memcmp` 5.0 % | the comparison build's load (mod parsing, as b_open_build) dominates; tree view 10 % |
| k_import (128) | `[libc]` 7.0 %, `pattern::do_match` 7.0 %, `pattern::find_bytes` 5.5 %, `LjTab<Key, TemplateVal>::find_node` 4.7 %, `Table::rawget` 3.9 % | build load 74 % (the imported build), recalc 20 %, tree view 16 % |
| l_save (625) | `pattern::do_match` 27.4 %, `memset` 15.4 %, `pattern::find_bytes` 9.9 %, `memcmp` 8.0 %, `pattern::classend` 4.5 % | build load 81.6 % (`open_from_list`); the saves themselves are 14-22 ms (XML compose) |

Recurring themes, in rough order of payoff:

1. **Lua-pattern string work in loading** (`pob_lua::pattern`): the mod parser on build load and
   `Item::ParseRaw`/`applyRange` for the item database; also the `memset`s next to it. Build opens
   cost 100-660 ms, the item database ~0.5 s after start-up.
2. **LuaJIT-table emulation** (`LjTab::newkey`/`find_node`, `Table::rawget`, `IndexMap` lookups,
   `Val` clones and drops) inside `initEnv` and `perform`: every recalc (20-40 ms) and every misc
   calculation (1.6-3.5 ms; a 4 s power report is on the order of 2000 of them).
3. **Allocation**: 9-13 % of the samples in recalc-heavy scenarios are malloc/free/memcpy;
   a recalc is 145k-285k allocations (`calc_<build>.full.allocs`).
4. **Tree view draw** and the per-frame `frame::digest`: 2.5 ms per idle tree frame, 31 % of
   `c_tree_alloc`.
5. **UI control tree walks** (`Cx::is_child_of`) in big tabs (Config: 8.6 ms frames).

## Limitations

- Headless, single-threaded app: no window system, no vsync, no OS event latency; image loads are
  synchronous (see "Images"). The `wgpu` mode measures the backend offscreen.
- The calc stage breakdown has no `offence`/`defence` timers (CCR-0150); use the sampling shares.
- Allocations are counted on the main thread only; the wgpu backend's threads are not.
- The governor cannot be pinned without root (see Noise); results record it.
- Coordinates in the scenarios are for 1920x1080 at dpi 1 and the current layout; a UI change that
  moves a control fails the scenario's guard, and the scenario needs new coordinates (see "Adding a
  scenario").
