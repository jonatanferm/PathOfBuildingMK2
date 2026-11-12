# CCR-0127: TP1 more than one LuaJIT oracle at a time (`POB_ORACLE_SLOTS`)

**Status: applied in part (CCRDOC, 2026-11-12; code `fa733f3` Merge port/TP1): `POB_ORACLE_SLOTS=N` (N flocks, slot 0 = `oracle.sh`'s lock) exists with **default 1**, which calc doc D20, AMENDMENTS A5 and testing.md now state. The default-3 change is rejected because it was not applied (`runner::oracle_slots` still returns 1, within the shared-machine memory budget).**

Unit: **TP1** (pob-oracle test speed). Filed status: **pending**; the code on `port/TP1` implements the
slots with the D20 default (1), so nothing changes until this CCR is applied. Number reserved with
`scripts/port/next_ccr.sh`.

## Contract

calc-architecture.md D20 (line 37): "`cargo.sh` / `oracle.sh` … `flock` a global lock (one
compile and one LuaJIT oracle at a time)", rationale "3 GB of free RAM … the LuaJIT oracle about
400 MB". AMENDMENTS A5 repeats it ("they flock one compile and one LuaJIT oracle machine-wide").
Implemented by `runner::OracleLock` (`crates/pob-oracle/src/runner.rs`, the `flock` on
`/tmp/pob-port-oracle.lock`) and `scripts/port/oracle.sh:60` (`flock "$lock"`).

## Why change it

* The machine now has 15 GB shared by ~11 agents; `scripts/port/cargo.sh` already runs up to 3
  test binaries at once (`POB_PORT_RUN_SLOTS`, default 3) under a 5 GB scope each, and every one
  of them that needs PoB queues on the single oracle lock.
* TP1 runs a test's builds on a bounded pool (`testkit::par_for_each`, `$POB_TEST_PAR`, default
  3). On a warm oracle cache no LuaJIT runs at all, but on a cold cache (after any edit of PoB, a
  patch, a stages file, an oracle script or the oracle runtime) the PoB side of every build still
  runs one at a time machine-wide, so the pool overlaps only the Rust side.
* One LuaJIT PoB under `jit.off()` peaks at ~400 MB (D20); measured test-process peaks with three
  pool workers stay well under the 3 GB per-process budget (see the TP1 measurements in
  `docs/port/testing.md`).
* Measured (quick corpus, cold cache via `POB_QUERY_CACHE=refresh`, `POB_TEST_PAR=3`): CO-C
  79.9 s with one slot → 47.5 s with three; BM-A 117.0 s → 34.9 s (peak RSS 1.43 GB). Full-corpus
  BM-A cold with one slot: 677 s wall for 214 s of CPU (waiting on the lock).

## Proposal

`POB_ORACLE_SLOTS=N` (default **3**, was 1): `OracleLock::acquire` holds one of N flocks,
slot 0 = `$POB_ORACLE_LOCK` (`/tmp/pob-port-oracle.lock`, still the one `oracle.sh` takes),
slot i = `<lock>.<i>`; tried in order without blocking, retried every 50-250 ms. `oracle.sh` keeps
taking slot 0 only (so a script run and N-1 test runs can overlap). Worst case N × ~400 MB of
LuaJIT machine-wide.

```diff
-| D20 | … `flock` a global lock (one compile and one LuaJIT oracle at a time). | …
+| D20 | … `flock` a global lock (one compile at a time; at most `$POB_ORACLE_SLOTS`, default 3,
+|     | LuaJIT oracles at a time, CCR-0127). | … the LuaJIT oracle about 400 MB each. …
```

and in `crates/pob-oracle/src/runner.rs` `oracle_slots()`: `.unwrap_or(1)` → `.unwrap_or(3)`.

## Until then

`POB_ORACLE_SLOTS` may be set per run (e.g. by the full suite at night, when the machine is
quiet). `// CCR-0127` marks the default in `runner::oracle_slots`.
