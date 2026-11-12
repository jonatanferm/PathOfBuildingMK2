# Testing the port

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
