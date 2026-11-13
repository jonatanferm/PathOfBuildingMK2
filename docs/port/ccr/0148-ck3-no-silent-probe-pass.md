# CCR-0148: CK3: no silent passes on probe failure, refresh once per process

Unit: **CK3** (hygiene; test-only; follow-ups of CK2's review, CCR-0147). Status: **applied**
by CK3. It adds to the `testkit` contract (contracts.md: `testkit`, owner ORC-X) and changes
what `POB_QUERY_CACHE=refresh` does in `testkit::probe_selection` / `testkit::oracle_dir`. No
signature of an existing function changes. The number was reserved with
`scripts/port/next_ccr.sh`.

## 1. No silent passes

**Problem.** CK2 made OF-F, DF-D, DF-F, PF-F and ORC-X probe their own dumps
(`testkit::probe_dumps`), but each still passed when the probe failed or dumped nothing:

- OF-F.rs `dumps()` printed `probe failed` and returned an empty map. A leg with no hit called
  `pob_testpaths::require`, which skips under `POB_ALLOW_SKIP=1`.
- DF-D.rs `run_corpus` listed a failed probe under "builds without dumps", which no gate
  checked. A corpus with no hit skipped.
- DF-F.rs: in all57, a failed probe or a build without a pair went to `missing`, which was not
  gated there (only `dff_l4_unit` checked it, and only when some hit ran). No hit at all skipped.
- PF-F.rs: a failed probe became `Verdict::Missing`, which was excluded from the gate.
- ORC-X.rs: in the round trip, a build whose `runSkillFunc` probe failed had its missing
  runSkillFunc dumps excused. `inject_is_typed` and `segment_runner_plumbing` turned a failed
  probe into a `require` skip.
- DF-G.rs (same pattern, same owner): a failed probe or a missing pair was `Missing`, which
  nothing gated. No build at all skipped.

**Change.**

- `testkit::oracle_required() -> bool` is `POB_ORACLE_REQUIRE=1` or `CI`, the condition the
  tests' `skip` helpers already used.
- `testkit::probe_inputs() -> Result<(), String>` returns `Err(what is missing)` when probing
  cannot happen: no `luajit` feature, no PoB checkout, or no `.parity` for the cache, and no
  `$POB_PROBE_DIR` override is set.
- Each test checks its inputs first: `data/dump`, `probe_inputs()`, the corpus. A missing
  input is the only skip. It goes through the test's `skip` (DF-D, DF-F, DF-G, PF-F) or a local
  `require` wrapper around `pob_testpaths::require` (OF-F, ORC-X, P3), and both assert
  `!oracle_required()`. Under `POB_ORACLE_REQUIRE=1` a missing input therefore fails even with
  `POB_ALLOW_SKIP=1`.
- After that check, these all fail the test: a probe that returns `Err`; a probe that dumps
  nothing (OF-F: none of the four probes; DF-D, DF-F: no pair; PF-F: one of the five segment
  dumps missing; DF-G: no `:3165` / `:end` pair; ORC-X: a gate-stage dump missing that is not
  listed, or any runSkillFunc dump missing outside `ORCX_RSF_MISSING`; P3: no hits index or no
  dump); and a run that compared nothing.
- OF-F also fails on a hit without its next dump, which it used to count and ignore.
- Each test prints the number of cases it compared and asserts that it is above zero.
- A `*_BUILDS` filter that selects no build of a curated corpus is reported and returns. It is
  an explicit narrowing, not a missing input.
- OF-F reads its dumps one hit at a time. Each dump's stack and env come from the run's
  `hits.json` instead of from loading every dump of a build together. Full-corpus peak RSS fell
  from 3.5 GB (CCR-0147 §5.6) to 2.4 GB, under the ~3 GB budget.

## 2. `POB_QUERY_CACHE=refresh`: once per process, never a partial entry

**Problem.** Under `refresh`, every `probe_selection` / `oracle_dir` call reran PoB and replaced
the entry with `remove_dir_all(entry)` followed by a rename. Several tests in one binary share
an entry: ORC-X's four corpus tests (one per build), the round trip and the runSkillFunc
replay, DF-G's three tests, and OF-F's two legs. Each of them rewrote the entry while another
read it. `remove_dir_all` deletes file by file, so a concurrent reader could find the
completion marker (`hits.json.zst` / `.complete`) next to dumps that were already gone.

**Change** (`testkit.rs`, which is not part of any cache key):

- `refresh_once(entry, filled, served, run)` (private to the crate) keeps a per-key slot for
  each process. The first caller of a key runs the refresh. Concurrent callers of the same key
  wait on the slot. Later callers are served the published entry while `filled()` holds. A
  failed run is not remembered, so the next caller runs it again. A unit test,
  `testkit::tests::refresh_once_runs_each_key_once`, starts 6 threads on one key and checks for
  1 run, 1 "ran" and 5 "served".
- `retire_entry(entry)` (private to the crate) renames the old entry to a private name with one
  rename, then deletes it. Under `refresh`, `probe_selection` retires the entry just before
  `SelectionCache::fill`, whose own `remove_dir_all` then finds nothing. `oracle_dir` retires it
  after the new run is staged and just before the rename that publishes it. A reader therefore
  sees the complete old entry, no entry, or the complete new one.

**Not changed (limit).** The swap is atomic for path lookups, but it does not protect a reader
in **another** process that has already resolved paths into the old entry. Two binaries that
refresh a shared entry at the same time, such as OF-E and OF-F, can still pull it out from
under each other. Fixing that inside `SelectionCache::fill_with` (runner.rs) would change
`runner.rs`. That file is part of every selection key (`PROBE_RUNTIME`), so every cached probe
on the machine would be re-probed, so the fix is left out. testing.md says to run `refresh`
from one binary at a time.

## 3. P3 off `.parity/probe`

`calc_diff_is_accepted_by_report_py` (its `--unit` provenance) and the ignored `all57_corpus`
now read the corpus probe run from the shared cache:
`testkit::probe_dumps(bin, xml, corpus/P3/stages.toml, ProbeOptions::default())`. This is the
default selection with `--max-dumps 200` and provenance on, which is what
`oracle.sh corpus --stages corpus/P3/stages.toml` ran. Before, they read the keyless
`.parity/probe/<stem>/`, which that command filled by hand. They read `hits.json[.zst]`,
`outputs.json[.zst]`, `provenance.json[.zst]` and `state/*.json[.zst]`. `POB_PROBE_DIR=.parity/probe`
still serves the old directory explicitly. `all57_corpus` stays `#[ignore]`d because it makes 57
probe runs under the P3 stages and needs `oracle.sh nondet`'s records.

## 4. Docs

`docs/port/testing.md`, the pob-oracle "oracle cache" section: the keyed entries (`sel-*`,
`dir-*`, `query-*`) and who uses them, `probe_dumps` / `BuildDumps` / `POB_PROBE_DIR`,
refresh-once, the no-silent-pass rule, the `POB_QUERY_CACHE` modes, `POB_QUICK_CORPUS` /
`POB_FULL_SUITE` / `POB_ORACLE_REQUIRE`, and "Open". The stale `testkit::dump_dirs` text is
gone.

## 5. Results

Each touched binary ran on its own (`POB_PORT_TEST_JOBS=1`, `POB_ORACLE_REQUIRE=1`, cache `on`)
from a worktree. The shared cache was warm, except for P3's new corpus entries, which this run
probed.

| test | quick (`POB_QUICK_CORPUS=1`, 14 builds) | full (`POB_FULL_SUITE=1`, 57 builds) |
|---|---|---|
| OF-F | passHit 333/333, combinePasses 324/324 (21 builds); 861 MB | 1169/1169, 1149/1149 (64 builds); 2.40 GB (was 3.5 GB) |
| DF-D | all57 95/95, unit 198/198, slice5 30/30 | 363/363 (57 builds), 198/198, 30/30 |
| DF-F | all57 14/14, unit 14/14 | 57/57, 14/14 |
| DF-G | slice5+unit 7/7, all57 14/14, no-breakdown 21/21 | 7/7, 57/57, 64/64 |
| PF-F | 15/15 builds pass | 58/58 |
| ORC-X | round trip 67 dumps (14/14/13/13/13), replay offence 13/13, runSkillFunc 13/13; 6/6 tests | 276 dumps (57/57/56/53/53), 56/56, 53/53; 6/6 |
| P3 (incl. ignored) | 8/8; all57_corpus: 14 runs, 961 dumps, provenance 11990/11990 | 8/8; 57 runs, 4002 dumps, provenance 45954/45954 |

Every test reported 0 failed or empty probes, 0 missing pairs and 0 "no next dump".

**Mutation check.** A temporary change made `probe_selection` return `Err` (`CK3_MUTATE=err`)
or an empty `state/` (`=empty`). It was applied, run and reverted, and is not committed. The
runs used the quick corpus with `POB_ALLOW_SKIP=1` and without `POB_ORACLE_REQUIRE`, so a skip
would have passed. Both mutations failed every test that probes, and each message names the
probe: OF-F 2/2, DF-D 3/3, DF-F 2/2, DF-G 3/3, PF-F 1/1, ORC-X 5/6, and P3's
`calc_diff_is_accepted_by_report_py` and `all57_corpus` 2/2. The ORC-X test that still passed
was `orcx_round_trip_synthetic`, which probes nothing.

**Missing input.** `POB_PARITY_DIR` was set to a path that does not exist, with
`POB_ALLOW_SKIP=1`. Without `POB_ORACLE_REQUIRE`, every test skipped: OF-F 2, DF-D 3, DF-F 2,
DF-G 3, PF-F 1, ORC-X 5, P3 2. With `POB_ORACLE_REQUIRE=1`, all of the same tests failed with
"required input missing".

**Refresh once per process.** This ran on a private parity directory with the 14 quick builds
and its own probe cache, so no shared entry was rewritten. A poller counted the
`pob_oracle probe` processes.

- Under `refresh`, DF-G started 21 probes, one per entry, for 3 tests that share them; without
  refresh-once it would start 42.
- ORC-X started 28 probes, 14 builds × 2 selections, for 6 tests; without refresh-once it
  would start 58.
- Both passed, and no `.retired` or `.tmp` directories were left behind.
- A second run under `on` started 0 probes and passed.
