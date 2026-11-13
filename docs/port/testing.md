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
* Not on the shared cache yet: PF-B (three dependent probe runs, the second pruned in place),
  CS3 (edits its patched copy after patching, so the patch sha does not describe it), MI,
  OF-G/H/I, PF-A, SK-B, TR-A/B, P3, U0 (per-worktree probe directories); the OF-A fuzz and the
  DF-A/CT/MP*/PS-*/LUT table queries run their LuaJIT side each time.
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
| `bridge-build-output` | the two pob-state documents of `LuaSession::load` + `dump_build_output` | `oracle_cache::pob_tree_key(PoB src)` (TP4: the relative path and BLAKE3 content hash of every PoB file but the jewel `.bin` caches, `JitPob`'s runtime `jit.rs`/`headless.rs`/`oracle.rs`/`oracle.lua`, mlua/LuaJIT versions, JIT mode), the pre chunk (state encoder; hashed with an empty root, see TP4), `BRIDGE_INIT_LUA`, build name, build XML, `DUMP_BUILD_OUTPUT_LUA` |
| `bridge-call-misc` | the document of `calcFunc(override, useFullDPS)` | as above, plus the override chunk, `useFullDPS`, `CALL_MISC_LUA` |
| `ui-<test>` | a test's Lua UI side (`LuaRun`: traces, error, snapshots, what the test reads from the VM or the user dir afterwards) | `ui_key`: `lua_side_code_hash` (TP4 set, see below), the test file's own source as compiled in (`test_source!()`: its Lua stubs and Lua-side wrapper), `pob_tree_key` of PoB `src/` and `runtime/`, the content of the fixture user dir and of the test's script directory (relative names and bytes, no paths), and the case parameters (script, screen, mode, stubs, query chunk, directives the script's steps do not show) |

The bridge keys do not include this crate's Rust code: the cached value is what the VM returned,
and everything after it (`StateDump::from_json`, injection, adoption) runs on every hit. A Lua UI
run that ends in an error is returned but not stored.

The `ui-*` keys are deliberately conservative: any edit to the code a Lua UI side runs
(`lua_side_code_hash`, TP4's set: this crate's `src/` without `lua_calc.rs`, `diff_report.rs`,
`oracle_cache.rs` and `bin/`, its `lua/`, `probe/`, `build.rs`, `Cargo.toml`; pob-sg, pob-host and pob-lua `src/` and
`Cargo.toml`; pob-oracle's `lib.rs`, `jit.rs`, `headless.rs`, `lua/` and `Cargo.toml`; the
third-party packages of `Cargo.lock`) or to the test file retires that test's entries, and the
next run recomputes its Lua sides (a "UI-cold, bridge-warm" run). The bridge keys survive such
edits; they change only with PoB, the oracle runtime and the build XML the Rust model saves.

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
`oracle/` cases after them, one at a time); TP4 converted the rest (see "TP4" below). Every UI
test that uses `LuaCalcBridge` gets the bridge cache without changes. To convert another test:
build the case list, run it through `par_map`, return per-case lines/failures and print/assert
them in order; wrap the Lua half in
`oracle_cache::cached_lua_run(oracle_cache::ui_key("ui-<test>", pob_ui_oracle::test_source!(),
root, &[fixtures], &[params]), || ...)`; a test that keeps a Lua side outside `par_map` holds an
`oracle_cache::Permit` instead of a per-binary lock.

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

## pob-ui-oracle: stable keys and the remaining binaries (TP4)

### Why the converted binaries were slow in the 2026-11-12 07:27 full suite

1. That run did not have the cache at all. `full_suite.sh` checks out rust-port when it starts:
   it started at 07:27:04 on f571987 (`Merge port/U46`, 07:13), and TP2 was merged three minutes
   later (91c97a3, 07:30:16). `git merge-base --is-ancestor 91c97a3 f571987` fails, and
   f571987 has no `crates/pob-ui-oracle/src/oracle_cache.rs`. Its u41 (1943 s), u32, u29, u21
   and u15 times are pre-TP2 times (slower than TP2's "before" column: the two lanes shared the
   machine with the pob-oracle crate and other agents). No later full suite had run when TP4
   started, so TP2 was never measured in the full suite.
2. With TP2 it would have missed most of the time anyway, for reasons in the keys:
   - `ui_key` hashed each fixture's path (`f.to_string_lossy()`): the test's script directory
     and the fixture user dir, both under `CARGO_MANIFEST_DIR`. The `../pob-fullsuite` worktree,
     the main checkout and every agent worktree had keys of their own, so entries an agent's run
     wrote were never hits for the full suite (and the reverse).
   - `lua_side_code_hash` covered all of `crates/pob-oracle/src` and the whole `Cargo.lock`. The
     Lua UI side uses only `jit` of pob-oracle (`boot.rs`), but the calc oracle's `diff.rs`,
     `inject.rs`, `patcher.rs`, `runner.rs`, a new bin, or any workspace dependency edit
     retired every `ui-*` entry: 9 of the 49 rust-port merges of 2026-11-11/12 (FUP3, INT-2,
     TP2, U54, TP1, CSPLIT, RETYPE, U45, U41), about every other full-suite run.
   - The bridge keys (and the PoB part of the `ui-*` keys) used
     `pob_oracle::runner::source_key`: the canonical path plus size and mtime of every PoB file.
     They happened to match across worktrees here (every worktree's `.PathOfBuilding` is a
     symlink to the main checkout's), but a touched or copied PoB tree missed everything, and
     the canonical root went in twice (raw and inside the pre chunk).
   - `ui_key` read the test file at run time: a test edited after its binary was built keyed
     the old binary's Lua side under the new source (a stale entry, not only a miss).

### Fixes (`oracle_cache.rs`, `lua_calc.rs::session_key`)

- `pob_tree_key(dir)`: content-addressed. Every file's path relative to `dir` and the BLAKE3 of
  its content (PoB's derived jewel `.bin`s left out, `patcher::is_derived_cache`), plus the
  oracle runtime (`jit.rs`, `headless.rs`, `oracle.rs`, `oracle.lua`, compiled in), the
  mlua/LuaJIT versions and the JIT mode. The per-file hashes are kept in a stat index
  (`<root>/stat-index/<hash of the dir>.json`: size, mtime, inode, when it was read) so only
  changed files are read again; a record is used only when the file's mtime is more than 2 s
  older than the read ("racy git"), and the index never decides the result: a moved or copied
  tree reads its files and gets the same key. Cold, the 770 MB of PoB's `src/` take a few
  seconds per process; with the index, a stat walk.
- `ui_key`: fixtures and script directories go in as `dir`/`file` plus their content hash
  (names relative to the directory), never their paths; the test file's text is the one
  compiled into the binary (`pob_ui_oracle::test_source!()`, `include_str!`).
- `lua_side_code_hash`: the code a Lua UI side runs (the TP2 table above): this crate's `src/`
  without `lua_calc.rs` (the bridge: keyed and cached on its own, never started inside a
  `cached_lua_run`), `diff_report.rs` (comparison after both sides ran), `oracle_cache.rs` (the
  cache itself, since fix round 1, see below) and `bin/`; pob-oracle's
  `lib.rs`, `jit.rs`, `headless.rs`, `lua/`; and only the third-party packages of `Cargo.lock`
  (`third_party_lock`: blocks with a `source`). It is still read from the worktree at run time,
  once per test process: an edit to a hashed file during a multi-binary run gives the binaries
  that start after it other keys than the ones before it (a warm run after that misses for the
  early binaries). Do not edit the crate while a measurement runs.
- `session_key`: no path. The pre chunk is hashed with an empty root: the root only shortens
  chunk names in the documents (`state_dump.lua` `S.relSource`, which falls back to the last
  `/src/` of a name), so the documents do not depend on it.

These change every key once: the first run after TP4 is cold.

Tests: `oracle_cache::tests::{keys_are_content_addressed, stat_index_rehashes_racy_files,
third_party_lock_keeps_registry_packages_only}`.

### Converted binaries

All of u16-u49 that run a Lua side are on the pool with the Lua UI side cached (`ui-uNN`
kinds): u16-u20, u22-u27, u30, u31, u33-u40 (u38: `ui-u38-unique`, `ui-u38-sd`), u42-u49,
besides TP2's u15, u21, u29, u32, u41. Same scripts, screens, case sets and comparisons; the
per-case output lines and failures are collected and printed in case order, then asserted as
before. Per-binary `static LOCK`s are gone: a scenario loop runs on `par_map`, every other test
that runs a PoB holds an `oracle_cache::Permit` (a case of the pool of its own). Parameters
that decide the Lua side but are not in the script's steps go into the key: `#!` directives
(`u20-setup`, `u20-panes`, saved files, `u35-settings`, `u42-visibility`, `u31-fake-calc`), the
net fixtures (u27, u43, u44), the compare import (u47, u49), `save_settings` (u16).

u27 keeps its one child process per run, one at a time (16 runs in one process passed the 5 GB
scope cap); each child's Lua side is cached, and the child prints its cache line as a `u27_`
line. Not converted: u50 and u52 (concurrent units U50/U53 own them), `tests/parity.rs` (TP2:
pool, no Lua cache).

### Fix round 1

- **VERIFY compared `extra` wrongly.** Most `extra`s are JSON (`serde_json::to_string` of what a
  test read back: u20, u22, u31, u33-u37, u43-u45 save `Builds/*.xml` texts into it), so the XML's
  quotes are escaped (`mainSocketGroup=\"3\"`). `same_text_unordered` splits at `"`, so every
  name part ended in `=\` instead of `=`. It never found an attribute and fell back to an exact
  compare, and PoB's `pairs`-order attributes failed the check (4 u45 entries in review). Now
  `same_extra_unordered` decodes two JSON texts and compares them as values, with every string
  leaf compared by `same_text_unordered`; anything else (u16's `Settings.xml`) is compared as
  text (test `unordered_extra_decodes_json_first`).
- **Multi-line attribute values defeated the text normaliser too** (found by the VERIFY run
  below: every u27 entry failed). u27's saved `TimelessData searchList` holds line breaks. With
  the attributes in `pairs` order the break falls between other attributes, so splitting at
  every `\n` gave different lines per run. `text_lines` now splits only outside `name="..."`
  values.
- **`oracle_cache.rs` is out of `lua_side_code_hash`**, like `lua_calc.rs` and
  `diff_report.rs`. It holds the cache, its keys, the pool and the VERIFY comparisons: they
  decide where a Lua side's result is kept and when it runs, never what it returns. A change
  to what the keys must cover bumps `EPOCH`. Each of this round's verifier fixes had retired
  every UI entry, a 2.4 h cold crate run.
- **u17 and u18 failed every warm run.** Their Lua sides draw the user dir's path (the Options
  dialog's build path tooltip, the build list's file paths), and `UserDir::new` is
  `<tmp>/pob-ui-oracle/<pid>-<n>`. A cached Lua side showed another process's path, and the
  path's text width also moves the layout. The cold runs passed, because both sides ran in the
  same dir. Each case of these two tests now runs in a fixed dir,
  `<tmp>/pob-ui-oracle/named/<case tag>`, under an exclusive `flock` on `<tag>.lock`, so a
  second process that runs the same case waits. The path also goes into the key
  (`case_user_dir` in each test). Every other converted binary passed warm, so no other Lua side
  shows its path.
- **Memory.** In the first cold run after TP4, u25 (4032 MB) and u29 were SIGKILLed by
  `oomguard`. u26 reached 4057 MB, u23 3318 MB and u21 3395 MB; before TP4, serially, they took
  2.0-2.6 GB. `limit_par(1)` did not help: u26 serially still peaked at 4110 MB, and at 4056 MB
  with `POB_ORACLE_NOCACHE=1`. The growth came from glibc arenas. Each case thread works in an
  arena, and an arena keeps its peak heap. The in-process `M_ARENA_MAX` cap comes after libtest's
  test threads have their arenas (u26 serially with a cap of 1: 4123 MB). With
  `MALLOC_ARENA_MAX=1` in the environment u26 took 2385 MB. Now the pool calls `malloc_trim(0)`
  when a case ends (`release_free_heap`, in `CasePool::give`, before the case's growth is
  measured). u26: 2397 MB, same time. A miss also no longer holds the Lua side's JSON text. The
  value is streamed through BLAKE3 into the zstd encoder, and the round-trip check decodes the
  compressed stream and compares the BLAKE3 of the re-serialisation (`encode_json_checked`). The
  entry format is unchanged. TP2's `to_vec` + `from_slice` + `to_vec` held two values and two
  texts at once, and a u25 entry is up to 30 MB compressed. On a miss the computed value is
  returned. In the final cold run every binary but u21 (3077 MB) and u25 (3156 MB) stays under
  3 GB (table below). No binary was killed in it.
- **u26's Lua side is not cached** (it was not cached under TP4 either: no `ui-u26` entries).
  With a power report of all-zero power, PoB's heat map draws `SetDrawColor(NaN, ...)`
  (PassiveTreeView.lua:828-1018: 0/0). JSON writes NaN as `null`, so the trace does not read back,
  and the cache now says so: `pob-ui-oracle cache: ui-u26/... not stored: its JSON does not read
  back (invalid type: null, expected f64 ... "SetDrawColor":[null`. u26 warm (156 s) is
  therefore its cold time. FOLLOWUPS "TP4" has the fix.

### Measurements (TP4 fix round 1, on rust-port ae0dc52)

The whole pob-ui-oracle crate ran one binary at a time (`POB_PORT_TEST_JOBS=1`) under
`scripts/port/cargo.sh test -p pob-ui-oracle --no-fail-fast`, with the full suite's environment
(`POB_FULL_SUITE=1`, `POB_ORACLE_REQUIRE=1`, `POB_PARITY_DIR`/`POB_ROOT`/`POB_DATA_DIR` of the
main checkout; these binaries read `fixtures/user1` and their own `tests/uNN`, so the full and
the quick corpus give the same runs). Cold means a fresh `POB_UI_ORACLE_CACHE` root, so the UI and
bridge entries were both cold. Warm means the same root again. VERIFY is
`POB_ORACLE_CACHE_VERIFY=1` on that warm root. Time and peak RSS are the `cargo.sh: bin-time`
values. The machine is shared with ~10 agents, and a full suite of its own ran during the
first cold run. The "07:27 full suite" column is the last full suite (binary times, no cache: it
ran before TP2's merge, see above; u37 was killed there). The parenthesised RSS is from the
first cold run after TP4, before this round's memory fixes. u17/u18 cold and warm come from a
rerun after their user-dir fix (their earlier entries were keyed without the path). Two changes
came after these runs. One is the line-break fix and the `oracle_cache.rs` exclusion from
`lua_side_code_hash` (verify and keying only, see VERIFY below). The other is the merge of
rust-port up to e0e39e5 (BRAND's `VERSION` in the hash, U53's `RustCalc` harnesses). After
the merge, cold, warm and VERIFY passed on a fresh root for u16, u17, u18 and u45: cold 169 /
72 / 94 / 47 s, warm 41 / 24 / 31 / 21 s, VERIFY 67 entries with 0 mismatches, peak RSS at
most 1692 MB.

| binary | 07:27 full suite (no cache) | cold | warm | VERIFY | peak RSS cold / warm (first TP4 cold) |
|---|---|---|---|---|---|
| parity | 156 s | 89 s | 89 s | - | 1288 / 1267 MB (1401) |
| u15_lua_calc | 611 s | 355 s | 46 s | 310 s | 2015 / 2341 MB (2563) |
| u16_main | 197 s | 151 s | 63 s | 151 s | 1433 / 1254 MB (1736) |
| u17_popups | 115 s | 71 s | 23 s | 71 s | 1339 / 1363 MB (1775) |
| u18_build_list | 154 s | 93 s | 30 s | 92 s | 1438 / 1493 MB (1909) |
| u19_build | 137 s | 84 s | 37 s | 89 s | 2022 / 1718 MB (2161) |
| u20_build | 669 s | 111 s | 40 s | 222 s (rc 101) | 1914 / 1698 MB (2145) |
| u21_stats | 882 s | 400 s | 154 s | 700 s | 3077 / 2858 MB (3395) |
| u22_notes | 121 s | 54 s | 22 s | 101 s | 1696 / 1667 MB (1896) |
| u23_tree_view | 461 s | 330 s | 146 s | 352 s | 2636 / 2430 MB (3318) |
| u24_tree_tooltips | 569 s | 226 s | 46 s | 332 s | 2058 / 1892 MB (2331) |
| u25_tree_tab | 468 s | 390 s | 205 s | 502 s | 3156 / 2871 MB (4032 killed) |
| u26_power | 248 s | 169 s | 156 s | 162 s (rc 101) | 2205 / 2200 MB (4057) |
| u27_timeless | 556 s | 585 s | 314 s | 333 s (rc 101) | 1623 / 1569 MB (1632) |
| u29_gem_select | 1322 s | 535 s | 66 s | 1787 s (rc 101) | 2798 / 2659 MB (3029 killed) |
| u30_skills | 325 s | 129 s | 33 s | 254 s (rc 101) | 1817 / 1800 MB (2046) |
| u31_skills | 550 s | 161 s | 40 s | 418 s | 1950 / 1909 MB (2303) |
| u32_items | 1325 s | 425 s | 43 s | 538 s | 2099 / 1930 MB (2195) |
| u33_items_ctor | 224 s | 83 s | 31 s | 118 s | 1800 / 1675 MB (2046) |
| u34_items_core | 681 s | 196 s | 62 s | 318 s | 1960 / 1721 MB (2147) |
| u35_display_item | 1766 s | 793 s | 51 s | 1468 s | 2050 / 1949 MB (2187) |
| u36_craft_a | - | 131 s | 22 s | 575 s | 1754 / 1573 MB (2167) |
| u37_craft | 106 s (rc -15) | 835 s | 43 s | 2004 s | 1940 / 1775 MB (2509) |
| u38_tooltips | 559 s | 351 s | 28 s | 262 s (rc 101) | 2233 / 1561 MB (2043) |
| u39_calcs | 267 s | 127 s | 41 s | 240 s | 2349 / 2249 MB (2443) |
| u40_breakdown | 251 s | 106 s | 48 s | 212 s | 2301 / 2063 MB (2469) |
| u41_config | 1944 s | 335 s | 48 s | 384 s | 1828 / 1921 MB (2103) |
| u42_config | 589 s | 178 s | 50 s | 321 s | 2111 / 1872 MB (2415) |
| u43_import | 268 s | 93 s | 38 s | 103 s (rc 101) | 1851 / 1835 MB (2104) |
| u44_character | 260 s | 101 s | 28 s | 67 s (rc 101) | 1713 / 1457 MB (1957) |
| u45_party | 210 s | 86 s | 23 s | 95 s | 1632 / 1417 MB (1831) |
| u46_compare | - | 165 s | 51 s | 313 s | 2459 / 2348 MB (2754) |
| u47_compare | - | 160 s | 59 s | 281 s | 2505 / 2359 MB (2745) |
| u48_compare_power | - | 44 s | 23 s | 82 s | 1291 / 1214 MB (1597) |
| u49_compare | - | 251 s | 82 s | 314 s | 2855 / 2176 MB (2805) |
| u50_compare_draw_b | - | 152 s | 130 s | - | 1443 / 1443 MB (1488) |
| u52_ext_build_list | - | 103 s | 103 s | - | 1044 / 1091 MB (1052) |
| total | 15990 s | 8651 s | 2512 s | | |

- Crate total, one binary at a time: cold 8651 s (2.4 h), warm 2512 s (42 min). The 07:27 full
  suite took 15990 s for the binaries it ran. The warm target ("well under 1 h") holds serially.
  With `testrun.py`'s default `POB_PORT_TEST_JOBS=3`, free run slots make it shorter. Warm, u27
  (314 s: 16 child processes, Rust-bound timeless generation), u25 (205 s), u26 (156 s,
  uncached Lua side), u21 (154 s) and u23 (146 s) are the long ones.
- `POB_FULLSUITE_TIMEOUT` (default 14400 s per crate) needs no change for this crate. Warm fits
  with a wide margin. A cold crate run (8651 s here) happens after any edit to the
  `lua_side_code_hash` set, PoB or the oracle runtime, and it also fits, with 1.6 h to spare
  for slot waits. The per-crate timeout counts waiting time. If a cold pob-ui-oracle run ever
  shares the machine with a cold pob-oracle run (6585 s serially) and both wait for slots, raise
  the timeout to 21600 s for that run.
- VERIFY ran on all 34 cached binaries (u15-u49 without u28) on the warm root:
  26 passed and 8 failed. 27 of about 2,600 verified entries differed (13572 s). Every
  difference is either PoB not reproducing its own output from one process to the next, or one
  more verifier gap. None is a stale or wrong entry:
  | class | PoB source | entries (binaries) |
  |---|---|---|
  | line breaks inside an attribute value (verifier gap, fixed above) | u27's `TimelessData searchList` | 14 ui-u27 |
  | gem variant pick | SkillsTab.lua:472-478, `pairs(possibleVariants)` (Vaal Righteous Fire vs its AltX) | 1 each ui-u20, ui-u29, ui-u30 |
  | flask buff source | CalcPerform.lua:1773, `pairs(flasks)` (item tables as keys): the first flask with an identical mod names `source` | 3 bridge-build-output (u26, u29) |
  | unique flavour text | ItemsTab.lua:4578-4580, first of `pairs(flavourTable)` | 2 ui-u38-unique |
  | `masteryEffects` value order | PassiveSpec.lua:203, `pairs(self.masterySelections)` | 3 ui-u44 |
  | export code | the deflated, `pairs`-ordered build XML (u43's round trip) | 2 ui-u43 |

  The tests themselves accept every one of these: they passed cold and warm on the same
  entries, and u20's T4 compare sets the variant pick aside explicitly. The binaries that failed under VERIFY did so only because the verify error aborted
  their Lua or bridge side: u26 and u30 then report Rust-side divergences. After the line-break
  fix, cold + VERIFY on a fresh root passed u27 (16 entries), u45 (the review's 4 failing
  entries: 26 hits, 0 mismatches) and u16 (`Settings.xml` text `extra`s) with 0 mismatches
  (1009 s). The other classes stay VERIFY failures, by design: the check is exact up to order,
  and these values differ between PoB runs. FOLLOWUPS "TP4" has the options.
