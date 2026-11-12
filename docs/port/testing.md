# Testing the port

How the oracle test suites stay fast. Sections by unit: TP1 (pob-oracle, below), TP2
(pob-ui-oracle), TP3 (runners: `scripts/port/cargo.sh`, `full_suite.sh`).

## Runners

Owner: TP3. Scripts: `scripts/port/cargo.sh`, `scripts/port/testrun.py`, `scripts/port/full_suite.sh`.
Tests: `python3 -m unittest discover -s scripts/port/tests` (`test_cargo_sh`, `test_testrun`, `test_full_suite`).

### `cargo.sh test`: test binaries in parallel

`cargo.sh test -p <crate> ...` runs in two phases, as before: `cargo test ... --no-run` under a compile
slot, then the run under a machine-wide run slot (`run.lock[.i]` in `$POB_PORT_LOCK_DIR`, default
`$XDG_RUNTIME_DIR/pob-port`; `POB_PORT_RUN_SLOTS`, default 3). The run phase now goes through
`testrun.py` instead of plain `cargo test`, which ran the test binaries of a crate one at a time:

- cargo still decides everything about each binary. `testrun.py` runs the same `cargo test` command
  with one extra `--config target.'cfg(all())'.runner=[testrun.py record <nonce>]`. cargo prints its
  usual `Running ...` line for each binary and starts the runner in that binary's environment, cwd and
  arguments (filters, `-- ...` args, `--quiet` for `-q`). `record` writes them on one marker line and
  exits 0, so cargo goes straight on to the next binary and then runs the doctests itself. rustdoc
  starts doctest executables through the same runner (`--test-runtool`); `record` execs those
  unchanged, as they are not `<target>/deps/<name>-<hash>` files.
- Up to `POB_PORT_TEST_JOBS` (default 3) recorded binaries run at once. The first runs under the run
  slot `cargo.sh` already holds; each further concurrent binary takes one more machine-wide run slot,
  without blocking, and only while no other `cargo.sh` waits for a run slot (waiting runs leave a
  `run.wait.<pid>` marker; a marker whose pid is gone is removed, so one left by a SIGKILLed
  `cargo.sh` blocks extra slots only if its pid has been reused). Each binary runs in its own `systemd-run --user --scope` with
  `MemoryMax=$POB_PORT_MEM` (5G) and `MemorySwapMax=$POB_PORT_SWAP` (1G). So, machine-wide, at most
  `POB_PORT_RUN_SLOTS` test binaries run, each under its own cap, as before. The cargo pass (which
  also runs the doctests) runs in a scope of its own under the first slot, alongside the first
  binary, so a `cargo.sh` has one more scope than run slots (the cargo pass is light once compiled).
- Output is deterministic: each binary's stdout+stderr goes to a spool file (in the target dir) and
  the blocks are printed in cargo's order, with the earliest unfinished one streamed live. A block is
  cargo's `Running` line, the binary's output, cargo's `error: test failed, to rerun pass ...` line
  when it failed (plus cargo's `Caused by: ... (signal: N, ...)` when it was killed), and then
  `cargo.sh: bin-time <target> <wall>s rss=<MB>MB rc=<rc>`. The doctest section comes last, then
  cargo's `error: N targets failed:` list (binaries in cargo's order, then doctests), then one
  `cargo.sh: test binaries: <n> run, <f> failed, <c> cancelled; wall ..., sum of binary walls ..., up to <k> at once`
  line. Progress lines (`cargo.sh: [i/n] ok 12.3s -p crate --test X`) go to stderr. A binary's
  stdout and stderr are both printed on stdout (one stream, in the order they were written).
- `rss` is the binary's peak RSS (`wait4`, the largest process of its tree). It has a floor of about
  20 MB (the forked runner before exec).
- `--no-fail-fast`: every binary runs; exit status 101 when any failed. Without it, cargo stops at the
  first failing binary in its order: here every binary after that one is cancelled (killed when it
  is running), the ones before it still run, and nothing after its block is printed. The doctests
  are not reported: the recording cargo pass runs them while the binaries run, so some or all of
  them may already have run, but that pass is killed when a binary fails and its output is dropped.
- Scheduling: the longest binaries start first, from the wall times of earlier unfiltered runs in
  `~/.cache/pob-port/test-times.json` (keyed by `quick|full` corpus and target; written under a
  flock with temp file + rename; unknown binaries count as long). This changes only when binaries
  start, never what is printed.
- `POB_PORT_TEST_JOBS=1` runs the binaries one at a time through the same runner (same output).
  `POB_PORT_PARALLEL_TESTS=0` makes the run phase plain `cargo test` again. `bench`, `--no-run`, and
  a `cargo.sh` without `testrun.py` beside it (or without `python3`) also run plain cargo.
- SIGTERM/SIGINT/SIGHUP (for example from `timeout`) are passed to every running binary (SIGKILL 5 s
  later); exit status 128+signal.
- Known differences from plain `cargo test`: without `--no-fail-fast`, when a binary later in
  cargo's order fails first (it started earlier, from the history) and then an earlier one fails,
  both count as failed in the summary line and the `targets failed` list (cargo would report only
  the first; the exit status is 101 either way); rerun hints always name the package
  (`-p pob-oracle --test CS3`; cargo omits `-p` in a single-package workspace); with `-q` (no
  `Running` lines) a hint names the binary file instead of the target; under `-v` the `Running` line
  shows the recording runner command; and the doctest section's own `Running`/`Finished` timing
  lines are cargo's from the recording pass.

### `full_suite.sh`: crates in concurrent lanes

`full_suite.sh` (timer unit `pob-port-fullsuite`, worktree `../pob-fullsuite`, full corpus,
`POB_ORACLE_REQUIRE=1`, per-crate `timeout ${POB_FULLSUITE_TIMEOUT:-14400}`) now tests the crates in up
to `POB_FULLSUITE_LANES` (default 2) concurrent lanes. The heavy crates (`POB_FULLSUITE_HEAVY`, default
`pob-oracle pob-ui-oracle`) start first, one lane each; the other crates of `POB_FULLSUITE_CRATES` follow
in list order in whichever lane is free. Inside each crate the test binaries run in parallel as
above. The memory bound is the machine-wide one: compiles hold a compile slot, every running test
binary holds a run slot and its own 5G scope, however many lanes are active.

Costs to other agents: each lane's `cargo.sh` takes its base run slot with the ordinary blocking wait
(the "only while nobody waits" rule covers the extra slots of `testrun.py` only) and holds it for the
whole run phase of its crate. With `POB_FULLSUITE_LANES=3` and the default 3 run slots the suite can
hold every run slot for the whole pob-oracle/pob-ui-oracle phase (1.5-2 h on the full corpus), and
other agents' `cargo.sh test` queue behind it; the default of 2 lanes leaves one slot free. The
per-crate timeout also counts time spent waiting for compile and run slots (behind sibling lanes or
other agents); full-corpus pob-oracle took 6585 s serially, so raise `POB_FULLSUITE_TIMEOUT` if
lanes run into it. The lane scheduling is tested on a fake `cargo.sh` (`test_full_suite`) only; it
has not yet run against real cargo. Lanes sharing one target directory do not stall each other's
compiles in the run phase: cargo releases the build-directory lock before it runs test binaries.

`summary.txt` keeps its format: the header line, one `<crate>: rc=<rc> <secs>s; <p> passed, <f> failed,
<i> ignored[; FAILED: <tests>]` line per crate (appended as crates finish, rewritten in
`POB_FULLSUITE_CRATES` order at the end), and the `finished` line. New, before `finished`:

    per-binary times (wall, peak RSS, exit status), slowest first:
      1409.2s 2104MB rc=0 pob-ui-oracle --test u29_gem_select

(from the `cargo.sh: bin-time` lines in each `<crate>.log`; ties in crate order).

### Measured (2026-11-11/12, pob-oracle, quick corpus, 71 test binaries, machine shared with ~10 agents)

| run | cargo.sh wall | run phase | binaries at once | sum of binary walls | results |
|---|---|---|---|---|---|
| new runner, probe cache cold | 7013 s | 6842 s | up to 2 | 7815 s | 167 passed, 12 failed, 5 ignored; 11 failing targets |
| old runner (`POB_PORT_PARALLEL_TESTS=0`), warm | 2256 s | - | 1 | 2225 s (libtest) | identical |
| new runner, warm | 4045 s (about 1010 s waiting for a run slot) | 3033 s | up to 2 | 3034 s | identical |

All three run slots were held by other agents' runs (and the full suite) almost all the time, so the
runner mostly had only the slot `cargo.sh` itself holds, and it got a second one only for short
stretches. As intended, it did not take more. The warm new run was also slower per binary than the
old one (CPU contention from the concurrent full suite: CS0 471 s vs 120 s of libtest time). So these
runs show no speed-up. With free slots the speed-up is bounded by `min(POB_PORT_TEST_JOBS, free run slots)`
and by the longest binary (`test_testrun` checks 3 binaries at once on a fake cargo). Per-binary peak
RSS ≤ 1.45 GB (PWR), largest process of the whole run 2.0 GB in both runners. The failing targets and
per-binary pass/fail/ignore counts were identical in all three runs.

## pob-oracle: the oracle cache and the corpus pool (TP1)

### The oracle cache

PoB's side of a pob-oracle test (LuaJIT, `jit.off()`) depends only on fixed inputs: the PoB tree
(and its patches), the oracle/probe scripts, the build XML and the call parameters. Every Lua
invocation the converted tests make goes through one content-addressed cache, so a warm run pays
only for the Rust side.

* **Where.** `<parity>/probe-cache/` of the main checkout (`.parity/probe-cache`;
  `.parity-quick/probe-cache` is a symlink to it). Worktrees find the main checkout's `.parity`
  (`oracle::find_parity_dir`), so every worktree, the quick and the full corpus and the full-suite
  worktree share one cache.
* **Entries** (`crates/pob-oracle/src/runner.rs`):
  * `QueryCache` / `cached_query` / `cached_query_result`: the text an in-process PoB query
    returns, at `<source-key>/<blake3(input)>/query-<blake3(key texts)>/result.zst`. Tests call
    `testkit::oracle_query(src, input, key, run)`, which takes the machine-wide oracle lock only on
    a miss. `<source-key>` (`runner::source_key`) hashes the oracle runtime (`jit.rs`,
    `headless.rs`, `oracle.rs`, `oracle.lua`), the `mlua` / `mlua-sys` / `luajit-src` versions of
    `Cargo.lock`, `ORACLE_CACHE_EPOCH`, the JIT mode, and the path and stat fingerprint of every
    file of the PoB tree (a patched copy's tree included). `input` is the build's XML bytes;
    `key` lists every other text that decides the result (prelude, scripts, build name, calls,
    a per-test version tag). Put every Lua string the run executes into `key` (BM-A:
    `POB_RUN_CALLS`; CS1 hashes its whole test file); the version tag is only for what `key`
    cannot see (e.g. Rust code that post-processes the result before it is cached).
  * `SelectionCache` / `testkit::probe_selection`: one `pob_oracle probe` run of a selection, at
    `<patched sha>/<build key>/sel-<blake3(selection, max dumps, provenance, runner.rs,
    bin/pob_oracle.rs, oracle runtime)>/state/<hit>.json.zst`. `<patched sha>`
    (`patcher::plan_sha`) covers the stages file, the patched sources and the probe runtime Lua.
  * `Oracle::run` (`outputs` / `stages` dumps of the `pob_oracle` binary) goes through
    `cached_query_result` too (keyed by the dump, the build's stem and `bin/pob_oracle.rs`).
* **Writes** are atomic: an entry is written under a private temp name (pid + sequence) and
  renamed into place, a directory entry as a whole; when concurrent writers (other tests, other
  worktrees) publish the same key, the first complete entry stands. A failed run is never cached.
* **Converted tests** (TP1): BM-A, CO-C, CS0, PWR (`oracle_query`); CS2, OF-D, PF-G, SK-A
  (`probe_selection`, replacing per-worktree `target/tmp/<unit>-probe/<stem>` directories keyed by
  the stem alone); CS1 (`testkit::oracle_dir`: a whole run directory, same keys and atomic
  publish); `oracle_harness` (`Oracle::run`). Already on it before TP1: BM-B, IT-A/B/C, MS-A/B
  (`QueryCache`), DF-E, OF-C, ORC-X (`probe_selection`). Tests that read the `oracle.sh`-filled
  probe cache (`testkit::dump_dirs`: DF-B, OF-B/E/F, PF-C/D/E/F, ...) run no Lua.
  Not cached on purpose: `runner::run_nondet` / `pob_oracle nondet` (they measure PoB's
  run-to-run differences, which a cache would hide). Not converted yet: see "Open" below.

### Environment

| Variable | Effect |
|---|---|
| `POB_ORACLE_NOCACHE=1` | bypass: every query and probe runs PoB; nothing is read or written (same as `POB_QUERY_CACHE=off`) |
| `POB_QUERY_CACHE=refresh` | run PoB again and overwrite the entries |
| `POB_QUERY_CACHE=verify` or `POB_ORACLE_CACHE_VERIFY=1` | run PoB like `off`, compare each fresh result with the cached entry of its key (query text; a selection's list of dump files, since dumps carry a per-process run id), print every difference (`oracle cache verify: … differs …`), write nothing; `runner::query_cache_summary()` reports the counts |
| `POB_TEST_PAR=N` | corpus pool workers (default 3; `1` = the serial loop on the test thread) |
| `POB_ORACLE_SLOTS=N` | LuaJIT PoBs allowed at once machine-wide (default 1, calc doc D20; CCR-0127) |

**Invalidate.** The keys cover the inputs, so editing PoB, a patch, a stages file, a probe or
oracle script, the oracle runtime files or the LuaJIT version retires the affected entries by
itself. To drop everything: bump `runner::ORACLE_CACHE_EPOCH`, or delete
`.parity/probe-cache/*/*/query-*` and `sel-*` (nothing else depends on them). To check a cache
against PoB: run the test once with `POB_QUERY_CACHE=verify` (or compare a run under
`POB_ORACLE_NOCACHE=1` with a cached run: the reports must be the same).

### The corpus pool

`testkit::par_for_each(items, f, consume)` (`src/par.rs`) runs `f(i, item)` on at most
`$POB_TEST_PAR` worker threads and calls `consume(i, item, result)` on the test thread **in input
order**, with workers at most 2×N items ahead (bounded memory). Tallying and printing in
`consume` gives exactly the serial report; a panic in `f` is re-raised when `consume` reaches
that item. Variants: `par_map`, `par_for_each_init` (per-worker state such as a LuaJIT state or
thread-local hooks: pob-calc's caches and hooks are thread-local, so a worker starts clean),
`par_for_each_bounded(items, in_flight, …)` (large results: at most `in_flight` items claimed and
not consumed). Adopting it is a one-line change of a `for build in &builds` loop whose body
returns its per-build tally.

What stays on the test thread: anything reading process-global state, chiefly the `unported!`
counters (`pob_calc::stub::stub_report`), which are process-wide (CCR-0128 proposes per-thread
counters). The timeless-jewel LUT table (`pob_gamedata::timeless_lut`) is process-wide too:
dropping it between builds (`clear_timeless_jewel_luts`, for RSS) goes through
`testkit::release_shared`, which runs it only when no multi-worker pool is running (the serial
path keeps the per-build drop); a pooled test drops it once after the pool. A clear inside `f`
races with another worker's `readLUT` (it locks the table twice) and fails that build's load. Those tests run only their Lua side on the pool (CO-C). OF-F was tried with its dump
loading prefetched (`par_for_each_bounded`, 2 builds in flight): 68 -> 60 s on the quick corpus
but 1.8 -> 2.5 GB peak RSS, too close to the 3 GB budget on the full corpus, so it stays serial.
Pooled: BM-A, CS2 (both tests), DF-B, CS0, CS1 (its thread-local filter counters are taken per
build and merged), PWR (`pwr_l5_replay`), OF-A (`ofa_l4_setup`'s child processes), CO-C (PoB
side).

### Measurements (TP1, 2026-11-11, aarch64, machine shared with ~10 agents)

libtest wall time per binary, peak RSS of the test process (rusage, incl. its children). "before"
= the rust-port binaries (`4870179`); CS2 and PWR then kept per-worktree dump directories, so
their second run was already partly warm. "cold" = empty cache (new keys), `POB_ORACLE_SLOTS=1`;
"warm" = second run; both `POB_TEST_PAR=3`. Quick corpus (14 builds; `.parity-quick`):

| binary | before (1st / 2nd run) | cold | warm | warm, `POB_TEST_PAR=1` | peak RSS before → warm |
|---|---|---|---|---|---|
| BM-A | 105.5 s | 117.0 s | **5.8 s** | 11.4 s | 705 MB → 756 MB (cold 1.08 GB) |
| CS2 | 181.4 / 75.1 s | 128.5 s | **30.6 s** | 75.0 s | 621 MB → 1.11 GB |
| DF-B | 136.6 s | 49.4 s | **46.5 s** | 128.2 s | 657 MB → 1.28 GB |
| CO-C | 81.6 s | 79.9 s | **18.6 s** | 18.4 s | 1.32 GB → 776 MB (cold 1.38 GB) |
| CS0 | 60.4 s | 51.7 s | **2.2 s** | 4.5 s | 909 MB → 996 MB (cold 1.46 GB) |
| PWR | 281.5 / 29.8 s | 307.4 s | **29.5 s** | 28.5 s | 2.14 GB → 1.27 GB (cold 2.11 GB) |
| OF-A | 72.3 s | 27.2 s | **27.4 s** | 72.0 s | 666 MB → 670 MB |
| CS1 | (170 s full suite) | 122.4 s | **37.7 s** | — | → 539 MB (cold 1.26 GB) |
| OF-D | (145 s full suite) | 379.9 s | **77.9 s** | — | → 874 MB |
| PF-G | (132 s full suite) | 138.7 s | **33.9 s** | — | → 476 MB |
| SK-A | (57 s full suite) | 744.7 s | **59.0 s** | — | → 432 MB |

Full corpus (57 builds; before = `.parity/fullsuite/20261111-1426/pob-oracle.log`):

| binary | before | cold | warm | peak RSS |
|---|---|---|---|---|
| BM-A | 683.3 s | 677.3 s (oracle-lock bound: 214 s CPU) | **30.0 s** | 731 MB warm, 980 MB cold |
| DF-B (no Lua) | 499.8 s | — | **211.4 s** | 1.99 GB |

* Determinism: every report (stderr, PoB's boot messages and timings dropped) is identical,
  line for line, between cold and warm, between `POB_TEST_PAR=1` and 3, and between before and
  after (BM-A, CO-C, CS0, DF-B, OF-A compared against the rust-port binaries); the pass/fail
  results are the same (PWR, OF-A, OF-F fail on the quick corpus before and after, with the same
  messages: quick-corpus gates). The full-corpus BM-A cold run found 28 entries the quick corpus
  had made (shared cache).
* LUT drop after the pool instead of per build (fix round 2; quick corpus, `POB_TEST_PAR=3`,
  peak RSS by rusage): BM-A cold (new key) 158.1 s / 1.06 GB, warm 6.0 s / 748 MB, `PAR=1`
  11.6 s / 383 MB; CS2 warm 30.3-30.7 s / 957 MB-1.08 GB, `PAR=1` 82.5 s / 610 MB. Reports
  identical between `PAR=1` and 3 (CS2: the same lines; its two tests interleave).
* `POB_ORACLE_SLOTS=3` (CCR-0127), cold (`POB_QUERY_CACHE=refresh`): CO-C 79.9 → 47.5 s, BM-A
  117.0 → 34.9 s (peak 1.43 GB).
* `POB_QUERY_CACHE=verify` (quick): CO-C 14 identical / 1 differing, CS0 6 / 9, BM-A 0 / 28;
  every difference printed is PoB's per-process `pairs` order (XML attribute order, node order,
  the `BossSkillAdditionalData` mods), which the tests already tolerate (all pass on fresh runs).
  `POB_ORACLE_NOCACHE=1` CS0: 62.7 s, report identical to the cached run.
* A cold entry is a one-time cost per key: after a change to PoB, a patch, the oracle runtime or
  the LuaJIT version, the first run of a probe-heavy test (SK-A, OF-D) is slower than before,
  where per-worktree directories keyed by the stem alone were reused (and never invalidated).

### Open

* Per-thread `unported!` counters (CCR-0128) would let OF-B/D/E/F, CO-C and the other
  stub-reading L4 tests pool their Rust side.
* Not on the shared cache yet: PF-B (three dependent probe runs, the second pruned in place),
  CS3 (edits its patched copy after patching, so the patch sha does not describe it), MI,
  OF-G/H/I, PF-A, SK-B, TR-A/B, P3, U0 (per-worktree probe directories); the OF-A fuzz and the
  DF-A/CT/MP*/PS-*/LUT table queries run their LuaJIT side each time.
* `POB_QUERY_CACHE=verify` compares `cached_query` texts and selection dump lists; an
  `oracle_dir` entry (CS1) is only rerun (like `off`), not compared.
