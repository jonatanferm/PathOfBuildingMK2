# Testing the port

How the oracle test suites stay fast. Sections by unit: TP1 (pob-oracle, below), TP2
(pob-ui-oracle), TP3 (runners: `scripts/port/cargo.sh`, `full_suite.sh`), INT-4 (crates and the
`mlua` gate, next).

## Runtime and dev-only crates, the `mlua` gate (INT-4)

INT-4 deleted the legacy `pob-engine`, `pob-data` and `pob-extract` (with its `pob_diff` binary
and `scripts/parity/run.sh`); parity against PoB is only the oracle suites below. AMENDMENTS A13.

- **Runtime crates** (no `mlua` in `cargo tree -e normal`, and `cargo check --target
  wasm32-unknown-unknown` green): `pob-lua`, `pob-gamedata`, `pob-modparser`, `pob-calc` and its
  six crates, `pob-sg`, `pob-sg-wgpu`, `pob-host`, `pob-ui-core`, `pob-app`, `pob-web`
  (`pob-desktop` is native-only, and its default normal tree has no `mlua` either).
- **Dev-only crates** (LuaJIT through `mlua`; never a normal dependency of a runtime crate):
  `pob-oracle` (the calc oracle; it is the docs' `pob-luahost`, AMENDMENTS A1), `pob-ui-oracle`
  (the UI oracle and `LuaCalcBridge`), and the build-time data tool `pob-dump`. `pob-testpaths`
  (test-input locator) is dev-only too, without `mlua`.
  `mlua` also appears as a dev-dependency of `pob-lua`, `pob-calc` and `pob-ui-core` (L0/HYG1
  tests against real LuaJIT).
- **The one opt-in path** from a shipping package to LuaJIT is `pob-desktop`'s non-default feature
  `lua-calc` (`pob-ui-oracle`'s `LuaCalcBridge`; `POB_UI_CALC=lua` selects the bridge's Lua side
  in the UI harnesses, which are dev-only too).
- **Gates** (CI's calc job runs the first two):

  ```sh
  for p in pob-calc pob-app pob-desktop pob-web; do
      echo "$p $(scripts/port/cargo.sh tree -p $p -e normal | grep -c mlua)"   # 0 each
  done
  scripts/port/cargo.sh check --target wasm32-unknown-unknown -p pob-lua -p pob-gamedata \
      -p pob-modparser -p pob-calc-core -p pob-calc-setup -p pob-calc-config -p pob-calc-defence \
      -p pob-calc-offence -p pob-calc-perform -p pob-calc -p pob-sg -p pob-sg-wgpu -p pob-host \
      -p pob-ui-core -p pob-app -p pob-web
  scripts/port/cargo.sh tree -p pob-desktop -e normal --features lua-calc | grep -c mlua   # 3: opt-in
  ```

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
invocation of a pob-oracle test, and every PoB dump one reads, goes through one
content-addressed cache, so a warm run pays only for the Rust side. No test reads a dump
directory that nothing keys (CK1/CK2/CK3, CCR-0147, CCR-0148).

* **Where.** `<parity>/probe-cache/` of the main checkout (`.parity/probe-cache`;
  `.parity-quick/probe-cache` is a symlink to it). Worktrees find the main checkout's `.parity`
  (`oracle::find_parity_dir`), so every worktree, the quick and the full corpus and the full-suite
  worktree share one cache.
* **Entries** (`crates/pob-oracle/src/runner.rs`, `testkit.rs`):
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
  * `SelectionCache` / `testkit::probe_selection` / `testkit::probe_dumps`: one `pob_oracle
    probe` run (a fresh process) of a selection, at `<patched sha>/<blake3(build XML)>/sel-<blake3(selection,
    max dumps, provenance flag, runner.rs, bin/pob_oracle.rs, oracle runtime)>/` with
    `hits.json.zst`, `outputs.json.zst`, `provenance.json.zst` and `state/<hit>.json.zst`.
    `<patched sha>` (`patcher::plan_sha`) covers the stages file, the patched sources, the probe
    runtime Lua (`state_dump.lua`, `provenance.lua`, `probe.lua`) and the Lua runtime versions.
    One entry is one PoB process, so an `:in` / `:out` pair never mixes runs. `probe_dumps(bin,
    xml, stages, opts)` returns a `BuildDumps` (`dirs()`, `path()`, `run_dir()`): the entry's
    `state/`, or `$POB_PROBE_DIR/<stem>/state/` when that override exists (an explicit,
    hand-made dump directory; e.g. `POB_PROBE_DIR=.parity/probe` serves what `oracle.sh corpus`
    left there). Helpers: `testkit::selection(hits)` (the `probe` defaults: `--max-dumps 200`,
    provenance on), `testkit::default_stages()` (`docs/port/stages.toml`). Readers accept
    `<name>.json.zst` (an entry) and `<name>.json` (an uncached run or an override).
  * `testkit::oracle_dir(src, input, key, fill)`: a whole directory a custom PoB run writes (CA,
    CS1, CS3, INT-3, PF-B), keyed like `oracle_query` and published as a whole (a `.complete`
    marker inside).
  * `Oracle::run` (`outputs` / `stages` dumps of the `pob_oracle` binary) goes through
    `cached_query_result` too (keyed by the dump, the build's stem and `bin/pob_oracle.rs`).
* **Writes** are atomic: an entry is written under a private temp name (pid + sequence) and
  renamed into place, a directory entry as a whole; when concurrent writers (other tests, other
  worktrees) publish the same key, the first complete entry stands. A failed run is never cached.
  Under `POB_QUERY_CACHE=refresh` (CK3, CCR-0148 §2) `probe_selection` and `oracle_dir` re-run
  each entry **once per process**: the first test to ask for a key re-runs it, concurrent tests
  of that key wait for it, later ones read that run (ORC-X's four tests share an entry per build,
  DF-G's three, OF-F's two legs). The old entry leaves the cache in one rename right before the
  new one is published, so a reader finds the complete old entry, none, or the complete new one,
  never one half deleted. (A reader in *another* process that already resolved paths into the old
  entry can still lose them mid-read; run `refresh` from one binary at a time.)
* **Users.** `oracle_query`: BM-A, CO-C, CS0, PWR. `probe_selection` / `probe_dumps`: AS, CS2,
  DF-A..DF-G, MI, OF-A..OF-I, ORC-X, P3 (corpus runs under `corpus/P3/stages.toml`), PF-A, PF-C..PF-G,
  SK-A, SK-B, TR-A, TR-B. `oracle_dir`: CA, CS1, CS3, INT-3, PF-B. `QueryCache` directly: BM-B,
  IT-A/B/C, MS-A/B. `oracle_harness`: `Oracle::run`. Not cached on purpose:
  `runner::run_nondet` / `pob_oracle nondet` (they measure PoB's run-to-run differences, which a
  cache would hide), and the self-checks that probe into a fresh per-process scratch directory
  (P3's `probe_one_build` / `two_runs_agree…`, U0). `oracle.sh run` / `oracle.sh corpus` /
  `bin probe --cache` still write the old `<sha>/<build-key>/` layout and `.parity/probe` for
  manual use; no test reads them.
* **No silent passes** (CK3, CCR-0148). A test skips only when an input is missing (`data/dump`,
  the PoB checkout, `.parity`; `testkit::probe_inputs()` names what probing lacks), and that skip
  fails under `POB_ORACLE_REQUIRE=1` / `CI` (`testkit::oracle_required()`; also with
  `POB_ALLOW_SKIP=1`). With the inputs there, a probe that fails or dumps nothing for a build fails
  the test, and every test asserts and prints how many cases it compared (> 0). Converted: OF-F,
  DF-D, DF-F, DF-G, PF-F, ORC-X, P3; earlier: AS, ORC-X's replay.

### Environment

| Variable | Effect |
|---|---|
| `POB_ORACLE_NOCACHE=1` | bypass: every query and probe runs PoB; nothing is read or written (same as `POB_QUERY_CACHE=off`) |
| `POB_QUERY_CACHE=off` | as `POB_ORACLE_NOCACHE=1`; a probe runs into a private directory under `target/tmp/probe-off/` (an `oracle_dir` run under `target/tmp/oracle-dir-off/`), deleted when the returned handle (`BuildDumps`, `ProbeDumps`, `OracleDir`) is dropped: hold it while reading |
| `POB_QUERY_CACHE=refresh` | run PoB again and overwrite the entries; probe and directory entries once per process (above) |
| `POB_QUERY_CACHE=verify` or `POB_ORACLE_CACHE_VERIFY=1` | run PoB like `off`, compare each fresh result with the cached entry of its key (query text; a selection's list of dump files, since dumps carry a per-process run id), print every difference (`oracle cache verify: … differs …`), write nothing; `runner::query_cache_summary()` reports the counts |
| `POB_PROBE_DIR=<dir>` | `probe_dumps` serves `<dir>/<stem>/state/` instead of the cache when it exists (test-specific overrides of the same kind: PF-C `$PFC_PROBE_DIR`, OF-B `$OFB_DUMP_DIR`) |
| `POB_QUICK_CORPUS=1` | the quick corpus: `.parity-quick` (14 builds) when `$POB_PARITY_DIR` is not set (`oracle::find_parity_dir`, `pob_testpaths::parity_dir`; CCR-0147 §5.5), and the tests' quick-corpus policy (`pob_testpaths::quick_corpus`: corpus-size and exact-coverage checks are full-corpus only). `cargo.sh test` sets it (with `POB_PARITY_DIR`) in the main checkout; worktrees run the full corpus unless it is set |
| `POB_FULL_SUITE=1` | `cargo.sh test` keeps the full corpus in the main checkout too |
| `POB_ORACLE_REQUIRE=1` (or `CI`) | a missing input fails instead of skipping (above) |
| `POB_TEST_PAR=N` | corpus pool workers (default 3; `1` = the serial loop on the test thread) |
| `POB_ORACLE_SLOTS=N` | LuaJIT PoBs allowed at once machine-wide (default 1, calc doc D20; CCR-0127) |

**Invalidate.** The keys cover the inputs, so editing PoB, a patch, a stages file, a probe or
oracle script, the oracle runtime files, `runner.rs` / `bin/pob_oracle.rs` (selection entries) or
the LuaJIT version retires the affected entries by itself. To drop everything: bump
`runner::ORACLE_CACHE_EPOCH`, or delete `.parity/probe-cache/*/*/query-*`, `dir-*` and `sel-*`
(nothing else depends on them). To check a cache against PoB: run the test once with
`POB_QUERY_CACHE=verify` (or compare a run under `POB_ORACLE_NOCACHE=1` with a cached run: the
reports must be the same).

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
counters (`pob_calc::stub::stub_report`), which are process-wide (per-thread
counters, CCR-0128, were not built: rejected, AMENDMENTS A14). The timeless-jewel LUT table (`pob_gamedata::timeless_lut`) is process-wide too:
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

* Per-thread `unported!` counters (CCR-0128, rejected as not built) would let OF-B/D/E/F, CO-C and the other
  stub-reading L4 tests pool their Rust side.
* Every dump a test reads comes from the shared cache since CK1/CK2/CK3 (above). Still run
  each time: the OF-A fuzz and the DF-A/CT/MP*/PS-*/LUT table queries (their LuaJIT side), and
  the fresh-scratch self-checks (P3, U0).
* `refresh` is once per process, not once per machine: two test binaries refreshing an entry
  they share (OF-E and OF-F; DF-A..DF-G do not share) can still remove it under each other's
  readers. Making the selection publish swap atomically across processes would change
  `runner.rs`, which is part of every selection key (every entry would be re-probed).
* `POB_QUERY_CACHE=verify` compares `cached_query` texts and selection dump lists; an
  `oracle_dir` entry (CS1) is only rerun (like `off`), not compared.

## pob-ui-oracle: Lua-side cache and parallel cases (TP2)

### Where the time went

Measured on rust-port 4870179 (8 cores, ~10 other agents), one fixture of `u29_dps_sort`
(`U29_DPS_FIXTURES=allflame_exp_001`): the Lua UI side took 7 s, the Rust side 195 s. Almost
all of the Rust side is `LuaCalcBridge` (`src/lua_calc.rs`): every `build_output` and every
`calcFunc` call boots a fresh LuaJIT PoB, loads the build XML and dumps the result, under the
machine-wide oracle `flock` that every agent's oracle runs share. The UI tests that use the
bridge (u20-u22, u29, u32, u39-u45, ...) spend most of their wall time there, and the rest in
the Lua UI side (a PoB boot, 3-8 s per case) and the Rust side. Rendering and comparing traces
is cheap (well under a second per case).

### The cache (`crates/pob-ui-oracle/src/oracle_cache.rs`)

Content-addressed, machine-wide: `~/.cache/pob-ui-oracle/<kind>/<k[0..2]>/<k>.zst`
(`$POB_UI_ORACLE_CACHE` overrides, else `$XDG_CACHE_HOME/pob-ui-oracle`). Every worktree,
the quick and the full corpus share it. An entry is a magic header, the BLAKE3 of the payload
and the zstd payload; it is written to a private temp name in its directory and renamed into
place, so concurrent writers (threads, processes, worktrees) each publish a whole entry and a
torn or corrupt entry reads as a miss and is rewritten. Errors are never cached.

What is cached and under which key (BLAKE3 over length-prefixed parts plus `EPOCH`):

| kind | value | key |
|---|---|---|
| `bridge-build-output` | the two pob-state documents of `LuaSession::load` + `dump_build_output` | `pob_oracle::runner::source_key(PoB src)` (stat fingerprint of every PoB file, `JitPob`'s runtime `jit.rs`/`headless.rs`/`oracle.rs`/`oracle.lua`, mlua/LuaJIT versions, JIT mode), the canonical root, the pre chunk (state encoder), `BRIDGE_INIT_LUA`, build name, build XML, `DUMP_BUILD_OUTPUT_LUA` |
| `bridge-call-misc` | the document of `calcFunc(override, useFullDPS)` | as above, plus the override chunk, `useFullDPS`, `CALL_MISC_LUA` |
| `ui-<test>` | a test's Lua UI side (`LuaRun`: traces, error, snapshots, what the test reads from the VM or the user dir afterwards) | `ui_key`: content hash of the Lua-side code (`crates/pob-ui-oracle/{src,lua,probe,build.rs}`, `crates/{pob-sg,pob-host,pob-lua,pob-oracle}/{src,Cargo.toml}`, `Cargo.lock`), the test file's own source (its Lua stubs and Lua-side wrapper), `source_key` of PoB `src/` and `runtime/`, the content of the fixture user dir and of the test's script directory, and the case parameters (script, screen, mode, stubs, query chunk) |

The bridge keys do not include this crate's Rust code: the cached value is what the VM returned,
and everything after it (`StateDump::from_json`, injection, adoption) runs on every hit. A Lua UI
run that ends in an error is returned but not stored.

The `ui-*` keys are deliberately conservative: any edit under the hashed crate directories (this
crate's `src/`, pob-sg, pob-host, pob-lua, pob-oracle) or to the test file retires that test's
entries, and the next run recomputes its Lua sides (a "UI-cold, bridge-warm" run). The bridge
keys survive such edits; they change only with PoB, the oracle runtime and the build XML the
Rust model saves.

Only the Lua side is cached; the Rust side always runs, so a change to the port is always
tested. The cached value is one sample of PoB's per-process nondeterminism (LuaJIT's seeded
string hashing: `pairs` order over string keys, STYLE §3.4), which the tests already tolerate.

Environment:

- `POB_ORACLE_NOCACHE=1`: no reads, no writes (every Lua side runs, as before TP2).
- `POB_ORACLE_CACHE_VERIFY=1`: every hit is recomputed and compared; a difference fails the
  operation and both payloads are kept in `<root>/verify-mismatch/<key>.{cached,fresh}`. The
  bridge documents compare after dropping `$id`/`$ref` targets, with lists as multisets and
  numbers to `REL_TOL` (PoB's own runs differ in mod order); Lua UI runs compare their traces up
  to call order within a frame (`diff_report::order_only_difference`) and their snapshots and
  `extra` as multisets of lines with sorted attributes (`SaveDB` writes in `pairs` order).
  These comparisons catch a stale or mis-keyed entry, not PoB's own order noise.
- `POB_UI_ORACLE_CACHE=DIR`: another root (e.g. an empty directory for a cold measurement).

Invalidation is automatic through the keys. To retire everything (e.g. after a change to
something outside the keys, like a system LuaJIT), bump `oracle_cache::EPOCH` or
`rm -rf ~/.cache/pob-ui-oracle`. Entries are small (a bridge document is ~100 KB compressed).

Each test prints a summary line: `pob-ui-oracle cache (On, <root>): H hits, M misses, B uncached
runs (T s in Lua runs)`.

### Parallel cases (`oracle_cache::par_map`)

`par_map(items, f)` runs `f` on each item on a fresh thread (256 MB stack; thread-local state
of the Rust side and the pob-app hooks therefore never leaks between cases), as many at once as
the process-wide case pool admits (`oracle_cache::Permit`; shared by every `par_map` call and
every test thread, so tests of one binary that run concurrently do not multiply the bound).
Results come back in item order; a panicking case is re-raised after all cases finished, the
first in item order. Do not nest `par_map` calls.

Admission (`CasePool`): the cases of one `par_map` call form a site (a `Permit::acquire`
holder on a test thread is a site of its own). A case starts when no case runs, or when fewer
than `POB_TEST_PAR` (default 3) run, its site's cases have been measured, and the process's
current RSS plus the largest growth one of them caused, counted once for the new case and once
for each running one (it may not have reached its peak yet), stays within `POB_TEST_MEM_MB`
(default 3000). Growth of a case = RSS (or the part of VmHWM above its start) at its end minus
RSS at its start; it also counts what concurrent cases allocated meanwhile (conservative). So
the first case of every site runs alone, and a site whose cases are heavy for the budget drops
to one at a time as the process grows. `POB_TEST_POOL_LOG=1` logs each admission.

The UI test processes keep most of what a case allocated (a u21/u29 case adds 0.6-1 GB on top
of the ~2 GB the data dump, timeless jewel tables and debug build take, and RSS climbs case after
case even serially). Running cases at once early therefore raises the whole run's peak: u21
reached 3.8-4.1 GB with the pool against 3.0 GB serially, so `tests/u21_stats.rs` calls
`oracle_cache::limit_par(1)` (process-wide, before any of its tests admits a case). Other heavy
binaries stay near 3 GB with the pool (numbers below); a new test binary that grows like u21
should call `limit_par(1)` too. Fixed worker counts measured on the way: u29 with 3 workers
peaked at 4.0 GB (cold) / 3.4 GB (warm), u21 with 2 workers at 4.2 GB (cold) / 3.9 GB (warm).

Every cache miss that boots a LuaJIT PoB (a Lua UI side in `cached_lua_run`, a bridge session in
`LuaSession::cached_*`) also holds one of `POB_TEST_LUA_PAR` (default 1) process-wide Lua VM
permits (`oracle_cache::LuaVmPermit`), the old "one LuaJIT PoB at a time" per process, while
other admitted cases' Rust sides run. A `cached_lua_run` closure must not start a bridge session
(it would wait for its own permit).

glibc malloc arenas: every case thread got an arena of its own, and an arena keeps its peak heap
after its thread is gone, so RSS grew with the number of threads ever run. The first pool use
caps the arenas at `max(2, POB_TEST_PAR)` (`mallopt(M_ARENA_MAX)`, glibc only, the crate's one
`unsafe` call; skipped when `MALLOC_ARENA_MAX` is set or with `POB_TEST_MALLOC_ARENAS=0`). u29
warm with 2 workers: 3343 MB peak without a cap, 2746 MB with `MALLOC_ARENA_MAX=2` from the
start, same wall time. Setting `MALLOC_ARENA_MAX=2` in the environment (cargo.sh) is a little
better than the in-process cap, which comes after the test threads have their arenas.

The old per-binary `static LOCK: Mutex<()>` ("one LuaJIT PoB at a time") in u21, u29, u32 and
u41 only bounded memory; each case has its own Lua VM (`mlua::Lua`, `Rc` state, the SG bridge's
registry is thread-local), its own user dir copy (`UserDir`, pid + counter), its own report
directory (by tag) and its own thread, so the pool replaces the lock. `LuaCalcBridge` VMs still
take the machine-wide oracle lock per operation, so a bridge-cold run is bound by that lock.

Converted call sites: u15 (reference sessions through `LuaSession::cached_*`), u21 (4 tests),
u29 (3 tests), u32, u41, and `tests/parity.rs` (gallery and scenario cases on the pool, the
`oracle/` cases after them, one at a time). Every UI test that uses `LuaCalcBridge` gets the
bridge cache without changes. To convert another test: build the case list, run it through
`par_map`, return per-case lines/failures and print/assert them in order; wrap the Lua half in
`oracle_cache::cached_lua_run(oracle_cache::ui_key("ui-<test>", file!(), root, &[fixtures],
&[params]), || ...)`.

### Measurements (TP2, rust-port 4870179 + TP2, 8 cores, ~10 other agents)

Before: the full suite of 2026-11-11 14:26 (`.parity/fullsuite/20261111-1426/pob-ui-oracle.log`,
binary time). These binaries read `fixtures/user1`, not the parity corpus, so the quick and the
full corpus give the same runs. After: `scripts/port/cargo.sh test -p pob-ui-oracle --test X --
--nocapture`, one binary at a time; cold = empty cache root, warm = the same root again; time =
the test binary's own (`finished in`); peak RSS = VmHWM of the test binary process, sampled.
Cold runs are bound by the machine-wide oracle lock (shared with every agent), so they vary a
lot with machine load.

| binary | before | cold | warm | peak RSS cold / warm |
|---|---|---|---|---|
| u29_gem_select (4 tests) | 1409 s | 1517 s | 105 s | 3195 / 3138 MB |
| u32_items | 1201 s | 406 s | 68 s | 2599 / 2290 MB |
| u41_config | 826 s | 309 s | 81 s | 2739 / 2453 MB |
| u21_stats (4 tests), pool without `limit_par` | 629 s | 471 s | 117 s | 4086 / 3840 MB |
| u21_stats, serial, nothing cached (pre-TP2 behaviour) | 629 s | 723 s | - | 2991 MB |
| u21_stats with `limit_par(1)` (final) | 629 s | killed | killed | - |
| u15_lua_calc (5 tests) | 444 s | 291 s | 39 s | 3099 / 3176 MB |
| parity (54 cases, no cache) | 213 s | 90 s | 112 s | 1174 / 1275 MB |

The final u21 runs were SIGKILLed twice by `scripts/port/oomguard.sh` (machine MemAvailable
below 1.2 GB from other agents' builds; a test binary at 1.9-2.8 GB was the largest process), so
the `limit_par(1)` numbers are not measured yet.

Same code, other worker settings (warm): u29 with 2 fixed workers 58 s (3.3 GB), with 3 fixed
workers 54 s (3.4 GB); u32 with 2 fixed workers 39 s; u41 with 2 fixed workers 45-61 s. The
pool trades those for memory. `POB_ORACLE_CACHE_VERIFY=1` on u41 (114 entries, UI and bridge):
all matched fresh runs.
