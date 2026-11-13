# Profiling suite of regular use (PROF1)

`pob-prof` replays what a user does in the app, on the real `pob_app::PobApp` with the Rust calc,
headless, and measures it: frame times, recalc latency, build load and start-up time, allocations
and peak RSS. Every repetition runs in a fresh process; the results are summarised as medians with
their dispersion, written as JSON, and compared against a committed baseline. Use it to hill-climb
on performance and to guard the idiomatic refactor against regressions.

It depends on no PoB code, no LuaJIT and no oracle crate (`cargo tree -p pob-prof | grep -c
"pob-oracle\|pob-ui-oracle\|mlua\|pob-testpaths"` is 0): only on the product crates (pob-app,
pob-host, pob-sg, pob-sg-wgpu, pob-calc, pob-gamedata, pob-lua, and pob-desktop's start-up library)
and on the files the product itself reads, from the in-repo asset bundle (`assets/`). It reads no
PoB checkout and no `data/` directory (see "Inputs"); pob-desktop's `tests/no_pob_checkout.rs`
scan covers this crate too.

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

The app reads what the desktop app reads (VENDOR): the asset bundle `assets/` (`pob_host::bundle`:
the script and SimpleGraphic runtime files through its `index.json`, the timeless jewel files
through `pob_desktop::BundleJewelFs`) and the compiled game data in `assets/gamedata` (`data.bin`,
`trees/`). It is resolved in one place, `crates/pob-prof/src/paths.rs`:

- **Asset bundle** (a directory holding `index.json`): `--assets DIR`, else `POB_PROF_ASSETS`, else
  `pob_desktop::locate_assets()`, the desktop app's own search: `POB_ASSETS` (the same meaning as
  for `pob-desktop`: a bundle directory), then `assets/` next to the executable, then this
  repository's `assets/` (compile-time; in a worktree, the worktree's own committed copy). A
  directory without `index.json` is refused. As in `pob_desktop::host_config`, the bundle is
  `GetScriptPath()` and SimpleGraphic's runtime directory (`GetRuntimePath()` its parent), and
  pob-host's `HeadlessFs` resolves the app's script-relative reads through the index.
- **Game data**: `--data DIR` (a directory with `data.bin`), else `<bundle>/gamedata`.
- **Timeless jewel tables**: inflated once and cached where the desktop app caches them
  (`pob_desktop::lut_cache_dir`, `~/.cache/pob-rust-port/timeless/<snapshot>`); the warm-up
  repetition of a fresh machine pays for the inflation.
- **Tests** (`cargo.sh test -p pob-prof`) use the same search with no overrides (the repository's
  bundle) and check that the inputs are the bundle, not a PoB checkout. Checked with the PoB
  checkout and `data/` hidden: `bwrap --dev-bind / / --tmpfs <main>/.PathOfBuilding --tmpfs
  <main>/data cargo.sh test -p pob-prof` passes (PROF1 fix round 2).
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

**Guards.** Every step driven by a click or a key carries a guard that fails the run when the
input misses: a recalc that must run, a tooltip that must compute stat differences, a tab or mode
that must be open, a file that must be written, a power report that must finish, a build
snapshot that must change, or a string that must (or must not) be drawn (`expect-text`, for
effects that are only visible: the Compare sub-views, the pinned Calcs breakdown, the selected
socket group, the generated and the pasted build code, the power report list, the tree search
text, the typed note). Each of these guards was checked by dropping the step's click or key from
a copy of the scenario: the run then fails in that step.

Most hovers, wheels and drags are guarded too (PROF1 fix round 2; each checked the same way, by
dropping one move, wheel or button press):

- `d_tree_navigate.hover_nodes`, `e_items.hover_slots`, `g_config.hover_options`: an `expect-misc
  N` after **each** hover, with the cumulative count of misc calculator calls (each tooltip
  computes its stat differences), so every single hover must land.
- `h_calcs.hover_cells`: an `expect-text` of the breakdown popup after each of the six hovers
  (the fifth hover now points at Mana Cost: the "Uses per second" cell it hovered before has no
  breakdown, CalcSections.lua:601, so that hover showed nothing and could not be told from a
  miss).
- `d_tree_navigate.zoom_in`, `zoom_out`, `drag`: the tree view's zoom and pan are part of the
  saved build (`PassiveTreeView:Save`: `zoomLevel`, `zoomX`, `zoomY`), so a `snapshot` after
  each wheel and after the drag must differ from the one before (`expect-differs`); the last
  zoom-out wheel is clamped at the outermost zoom and is checked to change nothing
  (`expect-same`).
- `d_tree_navigate.sweep`: `expect-misc 8`, the tooltips of the nodes it crosses. This one is
  an aggregate over 20 moves: a single move that lands elsewhere can still pass.
- `l_save.notes`: `expect-text "Profiling note: level changed"` after the typing (a missed click
  into the edit box or lost typing fails).

The steps below remain **unguarded**: their input is idle frames, a hover with no visible or
counted effect, or a wheel whose effect no guard can see (`expect-text` finds a string anywhere
in the frame's drawn strings, also when it is scrolled out of view, and a scroll position is not
saved in the build). They carry only `expect-no-recalc` / `expect-mode` (or `wait-idle`, which
fails when the background work does not end), which catch an unexpected recalc or a lost screen
but not a wheel or a hover that lands on nothing. **A miss there goes undetected**, and the step
then measures cheaper frames (`compare` reports it as `better`):

| scenario | unguarded step | what it does, and why no guard sees a miss |
|---|---|---|
| `a_startup` | background_load | idle frames until the item database loader is done (`wait-idle`) |
| `a_startup` | list_hover | hovers build list rows: only a row highlight, no string or count changes |
| `a_startup` | list_idle | idle frames on the build list |
| `b_open_build` | idle_tree | idle frames on the tree |
| `d_tree_navigate` | idle | idle frames on the tree (one move to the centre first) |
| `d_tree_navigate` | sweep | partly guarded (above): one missed move of the 20 can pass |
| `d_tree_navigate` | zoomed_out_idle | idle frames on the zoomed-out tree |
| `g_config` | scroll | wheel over the empty right side; at 1920x1080 the Config tab does not scroll there (`inspect --texts`: identical strings and positions before and after the wheel), so the step measures Config frames with wheel input whether the wheel lands or not |
| `h_calcs` | scroll | wheel over the sections; the Calcs tab scrolls, but its off-screen strings are still drawn, so no text guard sees the scroll position |
| `j_compare` | summary (the wheel part; the summary view itself is guarded) | wheel over the summary, as `h_calcs.scroll` |

A `better` on one of these steps deserves a look at `inspect --png` before it is believed.

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
| `heap_peak_mb` | peak live heap of the main thread during the step: an approximation, the main thread's net live bytes (allocated minus freed on that thread, `alloc.rs` keeps the counters per thread), so memory the main thread allocates and another thread frees (or the reverse) drifts it; the headless app does nearly all its work on the main thread |
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
`--threshold PCT` (default 10; the RSS fields `rss_peak_mb` and `total.peak_rss_mb` keep their
default of 20, see Noise: `--threshold` does not change them, only `--field-threshold
rss_peak_mb=PCT` / `peak_rss_mb=PCT` does),
`--field-threshold allocs=2` (repeatable), `--alpha 0.05`, `--gate wall_ms,allocs` (replace the gated fields), `--all` (print
every metric), `--scenarios a,b`. NEW is `INCOMPLETE` when a compared scenario has failed
repetitions in NEW, when a gated metric of BASE is absent from NEW (a step that no longer runs, a
build load that was not detected), or when a scenario named by `--scenarios` is missing from NEW;
a scenario missing from NEW without `--scenarios` is listed as "missing from NEW (not compared)"
but is no error (a subset run against the full baseline). Exit status: 1 when anything REGRESSED
or NEW is INCOMPLETE, 0 otherwise, 2 on usage errors. The last lines give the geometric mean of the `wall_ms` and `cpu_ms` ratios (new/base) over
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

**Heap profile**: `POB_PROF_FEATURES=dhat-heap scripts/perf/prof.sh exec <scenario> > /dev/null`
builds `pob-prof` with the `dhat-heap` feature (through cargo.sh with `POB_PORT_JOBS=1`, into the
usual target path, so `/tmp` worktrees work too), runs one repetition in the measuring scope and
writes `dhat-heap.json` in the repository root (view it with dhat's `dh_view.html`). The dhat build
replaces the counting allocator (its `allocs` read 0) and overwrites the measuring binary: the next
plain `prof.sh` call rebuilds it.

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
   `expect-differs A B`, `expect-same A B`, `expect-calc-func`, `expect-text "S"`,
   `expect-no-text "S"`. `expect-text` draws one extra frame with a trace recorder and looks for S
   in its drawn strings (escapes stripped; a substring match); that frame is not one of the step's
   frames and its time, allocations, heap peak and calc work leave the step (not its RSS high-water
   mark), but the app does run it.
2. Open the build in the set-up and end the set-up with `wait-idle`, so the measured steps do not
   include the background loading that follows every build open (measure that on purpose, as
   `b_open_build` does).
3. Find coordinates with `pob-prof inspect <your scenario> --until STEP --png /tmp/x.png --texts`
   (look at the picture; the strings list gives exact label positions, and a hovered tree node's
   tooltip names the node). Put a guard after every click and key, and check it: drop the input
   from a copy of the scenario, and `inspect` must fail in that step. A step whose miss cannot be
   seen goes into the table under "Guards".
4. `pob-prof inspect <scenario>` until it passes, then `cargo.sh test -p pob-prof` (every committed
   scenario must parse and have a description and a step; `committed_scenarios_cheap` replays
   `k_import`, `f_skills` and `j_compare` in a debug build (~20 s), and `-- --ignored` runs
   `committed_scenarios_all`, every scenario, ~3 min), then bless a new baseline (a new scenario
   is reported as "new scenario (not in BASE)" until then).

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
  heavier load `cpu_ms` is the one to trust. Allocation counts and (main-thread) heap peaks repeat exactly and are
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

`crates/pob-prof/baseline/baseline.json`: re-blessed in PROF1's fix round 1 from a clean commit
(2026-11-13T10:14Z, commit `faa9c31b`, not dirty, label "PROF1 fix round 1 baseline",
`--render none`, CPUs 2-7, governor schedutil, load 2.1 -> 1.1, 5 reps + 1 warm-up, 343 s;
within-run relative MAD of the steps' `wall_ms`: median 0.22 %, max 1.7 %). Against the first
baseline (run A of the noise test above, commit `0dbdab33` + the uncommitted unit) it is 0 of 732
gated metrics REGRESSED, geomean step `wall_ms` ratio 0.9971. The scenarios changed in that round
only by guards (untimed: the steps' frames and times are those of the first baseline, plus one
allocation per `expect-text`), except `h_calcs.pin_breakdown`, which gained a frame (the cursor
leaves the pinned cell for one frame before the unpinning click: 12 frames). Medians; `±%` is the
relative MAD; `load ms` is `build_load_ms`. Full numbers: `pob-prof show
crates/pob-prof/baseline/baseline.json`.

Start-up (median of all 12 app scenarios): `main` to the build list on screen **1.09 s** =
data.bin 380 ms + fonts 66 ms + `launch:OnInit` 582 ms + the first frame 57 ms; `exec` 0.9 ms.
After that the build list keeps loading the item database in the background for ~0.5 s
(`a_startup.background_load`, 14 frames of 28-50 ms). Peak RSS 551-589 MB per scenario process
(`i_power` the highest); the calc-only processes peak at 382 MB.

| scenario | step | wall ms (±%) | cpu ms | frames | frame p50 / p95 ms | recalcs (ms) | misc calls | allocs | load ms |
|---|---|---|---|---|---|---|---|---|---|
| a_startup | first_frames | 125.8 (0.2) | 125.5 | 3 | 34.72 / 56.49 | 0 (0.0) | 0 | 377,630 | - |
| a_startup | background_load | 508.5 (0.2) | 507.3 | 14 | 27.84 / 49.77 | 0 (0.0) | 0 | 1,654,313 | - |
| a_startup | list_hover | 0.3 (0.4) | 0.3 | 9 | 0.03 / 0.04 | 0 (0.0) | 0 | 1,618 | - |
| a_startup | list_idle | 0.8 (0.3) | 0.8 | 30 | 0.03 / 0.03 | 0 (0.0) | 0 | 5,376 | - |
| b_open_build | _setup | 632.4 (0.1) | 630.8 | 17 | 33.50 / 56.59 | 0 (0.0) | 0 | 2,031,942 | - |
| b_open_build | open_cold | 550.9 (0.6) | 549.4 | 5 | 2.42 / 540.91 | 1 (34.3) | 0 | 1,048,162 | 541.0 |
| b_open_build | open_warm | 109.8 (0.0) | 109.4 | 5 | 2.50 / 99.05 | 1 (30.6) | 0 | 766,203 | 99.1 |
| b_open_build | open_attack_old_tree | 674.5 (0.2) | 672.8 | 5 | 1.88 / 666.33 | 1 (29.0) | 0 | 1,739,265 | 666.4 |
| b_open_build | open_minion | 118.7 (0.1) | 118.3 | 5 | 2.46 / 108.04 | 1 (27.0) | 0 | 749,734 | 108.1 |
| b_open_build | open_dot | 114.7 (0.1) | 114.3 | 5 | 2.55 / 103.67 | 1 (19.6) | 0 | 675,293 | 103.7 |
| b_open_build | open_many_items | 153.8 (0.1) | 153.3 | 5 | 2.49 / 142.90 | 1 (36.3) | 0 | 860,058 | 143.0 |
| b_open_build | idle_tree | 49.2 (0.1) | 49.0 | 20 | 2.46 / 2.48 | 0 (0.0) | 0 | 275,065 | - |
| c_tree_alloc | _setup | 1,225.1 (0.5) | 1,222.0 | 20 | 30.61 / 121.60 | 1 (34.0) | 0 | 3,258,684 | 667.5 |
| c_tree_alloc | alloc_adjacent | 61.1 (0.4) | 60.7 | 7 | 2.57 / 38.80 | 1 (31.3) | 2 | 368,368 | - |
| c_tree_alloc | dealloc_adjacent | 52.9 (0.1) | 52.7 | 5 | 2.57 / 38.77 | 1 (31.3) | 1 | 328,576 | - |
| c_tree_alloc | alloc_path | 63.6 (0.1) | 63.2 | 7 | 2.66 / 39.32 | 1 (31.6) | 3 | 379,741 | - |
| c_tree_alloc | alloc_keystone | 61.4 (0.3) | 61.1 | 7 | 2.61 / 39.29 | 1 (31.6) | 2 | 369,310 | - |
| c_tree_alloc | dealloc_notable | 61.7 (0.8) | 61.4 | 7 | 2.66 / 39.50 | 1 (31.7) | 2 | 368,964 | - |
| c_tree_alloc | undo_redo | 156.0 (0.4) | 155.4 | 10 | 11.11 / 37.32 | 3 (95.3) | 0 | 993,480 | - |
| c_tree_alloc | dealloc_trunk | 101.1 (0.3) | 100.7 | 10 | 5.05 / 34.32 | 2 (48.6) | 5 | 580,828 | - |
| d_tree_navigate | _setup | 1,218.3 (0.5) | 1,215.1 | 17 | 33.75 / 121.39 | 1 (34.1) | 0 | 3,215,912 | 674.1 |
| d_tree_navigate | idle | 49.7 (0.4) | 49.5 | 20 | 2.47 / 2.51 | 0 (0.0) | 0 | 277,226 | - |
| d_tree_navigate | zoom_in | 15.0 (0.8) | 15.0 | 6 | 2.52 / 2.54 | 0 (0.0) | 0 | 84,113 | - |
| d_tree_navigate | hover_nodes | 59.0 (0.1) | 58.8 | 15 | 2.78 / 7.01 | 0 (0.0) | 7 | 301,234 | - |
| d_tree_navigate | sweep | 70.4 (0.3) | 70.2 | 20 | 2.58 / 7.31 | 0 (0.0) | 8 | 371,015 | - |
| d_tree_navigate | zoom_out | 27.3 (0.3) | 27.2 | 9 | 2.53 / 7.04 | 0 (0.0) | 2 | 149,224 | - |
| d_tree_navigate | zoomed_out_idle | 25.1 (0.5) | 25.0 | 10 | 2.51 / 2.54 | 0 (0.0) | 0 | 140,534 | - |
| d_tree_navigate | drag | 20.3 (0.3) | 20.2 | 8 | 2.53 / 2.58 | 0 (0.0) | 0 | 111,196 | - |
| d_tree_navigate | search | 31.1 (0.7) | 31.0 | 8 | 2.69 / 12.56 | 0 (0.0) | 0 | 204,934 | - |
| e_items | _setup | 1,238.6 (0.2) | 1,234.6 | 17 | 33.76 / 121.48 | 1 (39.1) | 0 | 3,306,612 | 694.0 |
| e_items | open_tab | 8.4 (0.5) | 8.3 | 5 | 0.85 / 4.49 | 0 (0.0) | 0 | 69,433 | - |
| e_items | hover_slots | 28.8 (0.1) | 28.7 | 10 | 3.48 / 6.13 | 0 (0.0) | 5 | 134,571 | - |
| e_items | swap_helmet | 64.6 (0.3) | 64.4 | 13 | 0.93 / 5.43 | 1 (36.5) | 3 | 432,629 | - |
| e_items | swap_helmet_back | 65.7 (0.1) | 65.5 | 12 | 0.94 / 5.35 | 1 (36.6) | 4 | 448,109 | - |
| e_items | item_set | 52.0 (0.1) | 51.8 | 12 | 0.88 / 2.85 | 1 (33.7) | 0 | 350,609 | - |
| e_items | item_set_back | 55.0 (0.1) | 54.9 | 12 | 0.91 / 2.82 | 1 (36.7) | 0 | 385,955 | - |
| e_items | weapon_swap | 86.2 (0.2) | 85.9 | 12 | 1.00 / 33.56 | 2 (63.8) | 0 | 642,064 | - |
| f_skills | _setup | 1,227.1 (0.8) | 1,224.1 | 17 | 33.85 / 121.56 | 1 (34.0) | 0 | 3,215,912 | 682.3 |
| f_skills | open_tab | 0.9 (0.6) | 0.9 | 5 | 0.16 / 0.24 | 0 (0.0) | 0 | 6,882 | - |
| f_skills | select_group | 3.5 (0.1) | 3.4 | 5 | 0.62 / 1.38 | 0 (0.0) | 0 | 33,060 | - |
| f_skills | gem_level | 76.5 (0.1) | 76.2 | 10 | 1.10 / 34.26 | 2 (61.7) | 0 | 566,500 | - |
| f_skills | gem_toggle | 83.3 (0.1) | 83.1 | 10 | 1.30 / 35.85 | 2 (61.5) | 3 | 612,134 | - |
| f_skills | add_support_type | 314.1 (0.2) | 313.3 | 7 | 50.96 / 57.44 | 0 (0.0) | 153 | 1,784,831 | - |
| f_skills | add_support_pick | 206.8 (0.2) | 206.2 | 4 | 52.02 / 57.52 | 1 (31.5) | 84 | 1,319,859 | - |
| f_skills | remove_support | 94.3 (1.7) | 94.0 | 6 | 2.15 / 51.75 | 1 (31.3) | 26 | 629,020 | - |
| f_skills | main_skill | 116.9 (0.2) | 116.6 | 24 | 1.95 / 35.58 | 2 (63.3) | 0 | 999,318 | - |
| g_config | _setup | 1,744.7 (0.2) | 1,740.1 | 17 | 33.80 / 120.82 | 1 (39.2) | 0 | 4,124,630 | 1,208.3 |
| g_config | open_tab | 26.8 (0.1) | 26.8 | 5 | 4.54 / 8.07 | 0 (0.0) | 0 | 152,596 | - |
| g_config | hover_options | 67.4 (0.3) | 67.2 | 10 | 8.56 / 9.15 | 0 (0.0) | 5 | 389,085 | - |
| g_config | toggle_onslaught | 150.4 (0.1) | 150.0 | 10 | 5.55 / 50.65 | 2 (78.2) | 4 | 963,850 | - |
| g_config | toggle_enemy_shocked | 158.5 (0.1) | 157.9 | 10 | 8.71 / 51.27 | 2 (78.9) | 5 | 987,480 | - |
| g_config | type_power_charges | 151.4 (0.2) | 151.0 | 13 | 4.85 / 47.28 | 2 (79.2) | 0 | 966,944 | - |
| g_config | scroll | 28.8 (0.2) | 28.7 | 4 | 9.83 / 9.83 | 0 (0.0) | 0 | 150,672 | - |
| h_calcs | _setup | 1,740.3 (0.3) | 1,735.9 | 17 | 33.78 / 120.86 | 1 (29.3) | 0 | 4,157,917 | 1,203.3 |
| h_calcs | open_tab | 5.9 (0.1) | 5.9 | 5 | 1.07 / 1.53 | 0 (0.0) | 0 | 41,028 | - |
| h_calcs | hover_cells | 13.9 (0.1) | 13.9 | 12 | 1.14 / 1.29 | 0 (0.0) | 0 | 103,290 | - |
| h_calcs | pin_breakdown | 13.2 (0.2) | 13.1 | 12 | 1.10 / 1.11 | 0 (0.0) | 0 | 93,837 | - |
| h_calcs | scroll | 6.8 (0.1) | 6.8 | 6 | 1.14 / 1.25 | 0 (0.0) | 0 | 48,959 | - |
| h_calcs | switch_group | 88.6 (0.0) | 88.3 | 24 | 1.11 / 31.60 | 2 (57.8) | 0 | 618,755 | - |
| i_power | _setup | 1,207.5 (0.1) | 1,204.6 | 17 | 33.81 / 121.41 | 1 (28.8) | 0 | 3,163,119 | 663.6 |
| i_power | heat_map | 4,049.6 (0.1) | 4,036.0 | 42 | 104.42 / 105.60 | 0 (0.0) | 0 | 23,085,981 | - |
| i_power | power_report_list | 29.0 (0.3) | 28.9 | 9 | 3.20 / 3.31 | 0 (0.0) | 0 | 98,501 | - |
| i_power | level_change_rebuild | 4,117.8 (0.1) | 4,104.4 | 42 | 105.00 / 106.05 | 1 (28.3) | 0 | 23,257,410 | - |
| i_power | heat_map_minion | 4,455.8 (0.5) | 4,440.8 | 48 | 105.17 / 106.26 | 1 (26.8) | 0 | 26,826,648 | 115.4 |
| j_compare | _setup | 1,219.0 (0.7) | 1,214.7 | 17 | 33.87 / 121.92 | 1 (34.1) | 0 | 3,215,912 | 671.7 |
| j_compare | open_tab | 5.5 (0.2) | 5.5 | 7 | 0.11 / 2.51 | 0 (0.0) | 0 | 32,092 | - |
| j_compare | import_comparison | 614.1 (0.2) | 612.4 | 14 | 0.37 / 1.06 | 1 (30.6) | 0 | 1,305,474 | - |
| j_compare | summary | 3.6 (0.1) | 3.6 | 6 | 0.57 / 0.69 | 0 (0.0) | 0 | 34,352 | - |
| j_compare | tree_view | 63.0 (0.5) | 62.7 | 10 | 3.21 / 40.97 | 0 (0.0) | 0 | 405,691 | - |
| j_compare | skills_view | 7.8 (0.2) | 7.8 | 6 | 0.43 / 3.34 | 0 (0.0) | 0 | 47,378 | - |
| j_compare | items_view | 17.3 (0.3) | 17.2 | 8 | 0.37 / 8.12 | 0 (0.0) | 6 | 117,425 | - |
| j_compare | calcs_view | 16.7 (0.5) | 16.6 | 8 | 2.42 / 3.76 | 0 (0.0) | 0 | 144,178 | - |
| k_import | _setup | 1,236.1 (1.6) | 1,232.3 | 17 | 34.00 / 121.99 | 1 (28.9) | 0 | 3,163,119 | 693.1 |
| k_import | open_tab | 5.5 (0.3) | 5.5 | 7 | 0.15 / 2.44 | 0 (0.0) | 0 | 32,688 | - |
| k_import | generate_code | 11.6 (0.1) | 11.6 | 5 | 0.30 / 10.70 | 0 (0.0) | 0 | 52,226 | - |
| k_import | paste_code | 3.9 (0.1) | 3.9 | 6 | 0.51 / 1.89 | 0 (0.0) | 0 | 7,374 | - |
| k_import | import_new_build | 120.8 (0.4) | 120.4 | 9 | 2.59 / 103.43 | 1 (26.7) | 0 | 768,423 | 104.4 |
| l_save | _setup | 632.9 (0.2) | 631.4 | 17 | 33.16 / 56.59 | 0 (0.0) | 0 | 2,031,942 | - |
| l_save | open_from_list | 538.9 (0.3) | 535.8 | 11 | 2.51 / 525.68 | 1 (21.8) | 0 | 970,483 | 525.8 |
| l_save | edit_level | 33.5 (0.3) | 33.3 | 5 | 2.44 / 23.66 | 1 (19.6) | 0 | 215,309 | - |
| l_save | save_ctrl_s | 21.7 (0.5) | 21.6 | 3 | 2.41 / 16.82 | 0 (0.0) | 0 | 103,653 | - |
| l_save | notes | 1.0 (0.2) | 1.0 | 9 | 0.11 / 0.14 | 0 (0.0) | 0 | 7,893 | - |
| l_save | save_button | 14.3 (0.3) | 14.3 | 5 | 0.10 / 13.92 | 0 (0.0) | 0 | 66,294 | - |

Calc stages per fixture build (`calc_<build>`, main-thread CPU ms; `full` is the app's recalc,
`buildOutput_MAIN` includes `initEnv` + `perform` + `calcFullDPS` + the cost-warning
`buildActiveSkill` passes):

| calc stages (ms, median of 5 iterations, median over reps) | full | initEnv | perform | buildOutput_MAIN | buildOutput_CALCS | getMiscCalculator | misc_call |
|---|---|---|---|---|---|---|---|
| allflame_exp_001 | 32.62 | 1.14 | 1.90 | 25.25 | 3.07 | 2.41 | 1.86 |
| allflame_exp_005 | 38.30 | 1.44 | 1.98 | 29.64 | 3.52 | 2.87 | 2.17 |
| allflame_exp_016 | 20.75 | 1.05 | 1.45 | 14.31 | 2.76 | 2.24 | 1.64 |
| allflame_exp_032 | 27.26 | 1.35 | 1.47 | 19.74 | 2.91 | 2.51 | 1.84 |
| allflame_exp_048 | 27.41 | 1.18 | 1.70 | 20.17 | 3.08 | 2.48 | 1.84 |
| upstream_Dual_Wield_Cospris_CoC | 40.90 | 1.31 | 3.52 | 30.14 | 4.76 | 4.03 | 3.51 |
| upstream_Generals_Perforate_Zerker | 30.47 | 1.05 | 3.08 | 22.70 | 2.54 | 3.46 | 2.95 |

A `wgpu` run of `d_tree_navigate` (Asahi Vulkan, offscreen 1080p, GPU waited for) gave tree frames
of ~8.5 ms p50 against 2.5 ms with `--render none`; the CPU rasteriser takes ~200 ms per frame.
No `wgpu` baseline is committed (record one with `--render wgpu` when working on the renderer).

## Hotspots (starting list for the performance work)

From `scripts/perf/flamegraph.sh <scenario>` on the first baseline's commit (`0dbdab33` + PROF1;
999 Hz, frame pointers, only the measured steps sampled; `--top 5`). Self time is the leaf frame; `[libc.so.6]` is libc
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
- Coordinates in the scenarios are for 1920x1080 at dpi 1 and the current layout. A UI change that
  moves a clicked control, a hovered tooltip target, a Calcs cell or the tree view fails the
  step's guard, and the scenario needs new coordinates (see "Adding a scenario"). A change that
  moves what one of the unguarded steps points at is not detected: `a_startup` background_load,
  list_hover and list_idle; `b_open_build` idle_tree; `d_tree_navigate` idle, sweep (partly
  guarded) and zoomed_out_idle; `g_config` scroll; `h_calcs` scroll; the wheel part of
  `j_compare` summary (the table under "Guards" says why). They then measure cheaper frames.
- Guards and snapshots are untimed (`driver.rs` `untimed`): their wall and CPU time, allocations,
  heap peak and calc work (an `expect-text` frame's recalcs and misc calls) leave the step. Only
  the RSS high-water mark cannot be put back: a guard frame that raises VmHWM raises the step's
  `rss_peak_mb`.
- `heap_peak_mb` is the main thread's net live bytes (see "What is measured"), an approximation
  of the process heap: memory freed on another thread than the one that allocated it drifts it.
