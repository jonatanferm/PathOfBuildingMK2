# Port testing: caches, parallel cases and runners

Infrastructure notes for the test suites (units TP1, TP2, TP3). Each unit owns its section.

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
