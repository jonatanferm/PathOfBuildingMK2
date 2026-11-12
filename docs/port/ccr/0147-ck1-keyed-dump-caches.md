# CCR-0147: CK1: keyed dump caches in the pob-oracle tests (closes CCR-0146 §4)

Unit: **CK1** (hygiene; test-only). Status: **applied** by CK1 for the tests in §2. The items
in §3 are still open and are left to their owners. Frozen contracts do not change. The number
was reserved with `scripts/port/next_ccr.sh`.

## 1. Problem

CCR-0146 §4: several pob-oracle tests reused PoB dumps from private directories under
`$CARGO_TARGET_TMPDIR` (`target/tmp/`) that had no content key, or were keyed only by the dump
format string. In a long-lived target dir (the full-suite worktree) they could keep passing
on dumps from an older harness. That is how MI passed in 36 s on pre-ORC-X2 dumps. Changing the
harness (`runner.rs`, `bin/pob_oracle.rs`, `state_dump.lua`, `probe.lua`), the patcher, the
stages, the PoB tree or the build did not invalidate them.

## 2. Change (applied)

Every test below now gets its dumps from the shared content-addressed cache
(`.parity/probe-cache/`), which `POB_QUERY_CACHE=off|refresh|verify` controls as it does for
the other users:

| test | was | now |
|---|---|---|
| SK-B.rs `probe_dumps` | `target/tmp/skb-probe/<stem>/`, reused if `hits.json` existed | `testkit::probe_selection` (`skill:*`, max 200, no stages) |
| TR-A.rs / TR-B.rs `probe_dumps` | `target/tmp/tr{a,b}-probe/<stem>/`, reused if `hits.json` existed | `testkit::probe_selection` (`triggers:*,perform:out`, max 200, no stages; TR-A and TR-B share the entry) |
| PF-A.rs `probe_into` (3 selections) | `target/tmp/pfa-probe{,-hits,-late}/<stem>/`, keyed by `format.txt` | `testkit::probe_selection` (`corpus/PF-A/stages.toml`, max 1000). `PFA_REPROBE=1` became `POB_QUERY_CACHE=refresh` |
| CA.rs `pob_run` (dump-cache code only) | `target/tmp/ca-probe/<stem>/`, keyed by `format.txt`, `error.txt` cached for good | `testkit::oracle_dir` |
| INT-3.rs `pob_run` | `target/tmp/int3-probe/<stem>/`, no key, `error.txt` cached for good | `testkit::oracle_dir` |
| OF-G.rs / OF-H.rs / OF-I.rs `ensure_dumps` | `$CARGO_TARGET_TMPDIR/OF-{G,H,I}/<stem>/`, reused if `complete` existed | `testkit::probe_selection` (`docs/port/stages.toml`, max 200) |
| PF-B.rs `build_dumps` (3 probe runs) | `target/tmp/pfb-probe/<stem>/`, reused if `done` existed | `testkit::oracle_dir` |

Keys:

- `probe_selection` uses `runner::SelectionCache`. Its key is `patcher::plan_sha` (patcher
  version, `state_dump.lua`, `provenance.lua`, `probe.lua`, the stages, the patched files, the
  Lua runtime versions, the PoB tree's stat fingerprint), the build's BLAKE3, the selection,
  the dump cap, the provenance flag, and the probe's Rust code (`runner.rs`,
  `bin/pob_oracle.rs`, `jit.rs`, `headless.rs`, `oracle.rs`, `oracle.lua`). Each entry holds a
  single probe process, so an in/out pair never mixes runs.
- `oracle_dir` (CA, INT-3, PF-B) is keyed by `runner::source_key` of the patched tree: the
  oracle runtime, the Lua runtime, the JIT mode, the canonical path (which contains the sha),
  and the tree's stat fingerprint. It also takes the build's BLAKE3 and these key parts: a tag,
  `patched.sha`, the build name, the test's Lua (`ca.lua` / `int3.lua`), `runner.rs`
  (`probe_prelude`), and the test code that makes the run. For CA and INT-3 that code is the
  span between `// ---- pob-run begin` and `// ---- pob-run end` (`pob_run_code()`), so edits
  elsewhere in CA.rs, for example FUP4's L5/L6 compare, do not retire the entries. For PF-B it
  is the whole of PF-B.rs plus `bin/pob_oracle.rs` and `PFB_WINDOW`. A failed run (`Err`) is
  not cached.
- CA still writes `target/tmp/ca-probe/<stem>/{ca.json | error.txt}` after each run, because
  `scripts/port/fresh_corpus_summary.py` and `fetch_fresh_corpus.py --probe` read it. The test
  only writes that directory and never reads it back.
- The dump caps and selections are the ones the old private runs used: the `pob_oracle probe`
  default of 200 where no `--max-dumps` was passed. No gate changed.

OF-G's fixed build list names 9 builds that are not in the quick corpus (`allflame_exp_000`,
`_005`, `_006`, `_013`, `_017`, `_029`, `_042`, `_045`, `_050`). Under `POB_QUICK_CORPUS=1` the
test failed with "missing test input". The fault was already on rust-port and is independent
of this change. Such a build is now noted as "not in the quick corpus" and does not count as a
missing input, which matches the `pob_testpaths::quick_corpus` policy (a filtered run). The
full-corpus gate is unchanged.

Readers now accept both `<name>.json.zst` (cache entry) and `<name>.json` (uncached run):
SK-B `hit_row`, TR-A/TR-B `run_file`. The others already did, or read through
`StateDump::load` / `testkit::find_dump`, which handle both. A test with no `.parity` directory
skips with a message (it fails under `POB_ORACLE_REQUIRE=1`), as the other cache users do.

## 3. Found by the sweep, not changed here (open)

`rg 'target/tmp|CARGO_TARGET_TMPDIR|hits\.json|format\.txt|complete|done'` over
`crates/pob-oracle/{src,tests}` and `crates/pob-ui-oracle/{src,tests}`:

1. **`testkit::dump_dirs`** (crates/pob-oracle/src/testkit.rs:105-125), used by PF-D, PF-E,
   PF-F, ORC-X (`load_dump`), OF-A, OF-D, OF-E, OF-F, DF-A (`load_dump`), DF-C, DF-D, DF-F and
   DF-G. It serves dumps from `.parity/probe/<stem>/state/` (a manual directory, 2026-10-09) and
   from `.parity/probe-cache/<every sha>/<build-key>/` (the pre-SelectionCache layout, with
   dumps directly in the build dir) of **every** patched-source sha, taken in alphabetical sha
   order rather than the current one. A dump from a retired harness can therefore be read
   whenever the current sha's directory lacks the hit. Proposed: restrict the probe-cache arm
   to the current `shared_patch(stages)` sha (the callers know their stages file), drop or opt
   in (`POB_PROBE_DIR`) the `.parity/probe` arm, and move the callers to `probe_selection`
   (OF-C/OF-D/DF-E did this). The callers depend on dumps they do not probe themselves, so this
   is a multi-owner change (testkit is HYG3/TP1's) with cold re-runs of ~13 tests.
2. **CS3.rs:104 `tmp_dir`** is keyed by `DefaultHasher(MODEL_LUA, stages.toml)` only, and
   `ensure_dumps` (CS3.rs:188) reuses `dumps/<stem>/` while `complete` exists. Changes to the
   dump Lua, the patcher, the runner or the PoB tree do not invalidate it. Proposed: wrap
   `ensure_dumps` in `testkit::oracle_dir` with the custom patched tree as `src` (its
   `source_key` covers the edited CalcSetup.lua by stat fingerprint, and its path the patcher
   sha), plus `runner.rs`, `bin/pob_oracle.rs` and the selection as key parts.
3. **DF-D.rs:178 `private_dir`** reads `target/tmp/dfd-probe/<stem>/state/` first. It is filled
   by hand with `oracle.sh exec probe` and never invalidated, and after it come the `dump_dirs`
   of item 1. Proposed: `probe_selection` with
   `buildDefenceEstimations:1681,buildDefenceEstimations:2153` on `docs/port/stages.toml`.
4. **OF-B.rs:364 `owned_root`** reads `.parity/unit/OF-B/<stem>/`, filled by hand with
   `corpus/OF-B/fill.sh`. The selection is checked against `SELECT`, but nothing keys the
   harness or PoB version. Proposed: `probe_selection` with `SELECT`, keeping `OFB_DUMP_DIR` as
   an explicit override.
5. Opt-in overrides that are explicit, so not changed: PF-C `$PFC_PROBE_DIR`, OF-B
   `$OFB_DUMP_DIR`, `$POB_PROBE_DIR` in `dump_dirs`.
6. Not dump caches: CS1 (already `oracle_dir`), IT-A/IT-B/IT-C (`QueryCache`; `data.bin` is
   the game-data dump), P3/U0 (fresh per-process scratch directories), PWR (`pwr-fixtures`,
   rewritten on every run). pob-ui-oracle has no ad-hoc dump cache: its oracle results go
   through `oracle_cache.rs` (TP4's).

## 4. Results

Every touched test was run cold, one test binary at a time (`POB_PORT_TEST_JOBS=1`), with
`POB_ORACLE_REQUIRE=1`. Every probe/oracle entry was re-made by this harness under
`POB_QUERY_CACHE=refresh`. SK-B's full run used `on` with entries that did not exist yet, so it
probed (335 s).

Full corpus (`POB_FULL_SUITE=1`, `.parity`, 57 builds):

- SK-B: 127 builds, 65/65 hooks covered, 385/385 dumps clean, 0 blocked.
- TR-A: 63 builds, 563/563 hits passed (handlers: default 148, CWC 12, Focus 3).
- TR-B: 98 builds, 885/885 hits passed, 82/82 configTable arms called by a passing hit.
- PF-A: all57 first calls clean (for example doActorLifeMana 58/58), hit sample 4 builds clean,
  late hits 24/24 clean with 6 filled-cache pairs. 4/4 tests passed.
- CA: L5 cost 56/56, used-tracking 57/57, CALCS lists 57/57. L6: 57 builds, 0 PoB failures,
  headline 741/741, all keys 45954/45954, CALCS 171/171, used-tracking 513/513, misc
  1248/1248. D13 passed. 5/5 tests passed.
- INT-3: breakdown 10606/10606 entries equal; tabulate 38940/38940 queries equal; 57 builds,
  0 PoB failures.
- OF-G: 105 ok, 0 failed. OF-H: 252 pass, 0 fail. OF-I: 58/58 hits clean.
- PF-B: 1238/1239 clean. The one remaining case is the documented, ungated "needs itemsTab
  slots" case (allflame_exp_039 initMinionModDB:in#6). 0 unpaired, 0 probe failures.

Quick corpus (`.parity-quick`, 14 builds, `POB_QUICK_CORPUS=1`):

- SK-B 385/385; TR-A 526/526; TR-B 844/844 (82/82 arms); PF-A 4/4 tests passed.
- CA: L6 11990/11990 keys, headline 182/182. INT-3: 2738/2738 entries, 9900/9900 queries.
- OF-G 42 ok; OF-H 145 pass; OF-I 18/18; PF-B 307/308 (the same ungated case).

No test went red once it stopped using the old directories, so no harness or calc fix was
needed.
