# CCR-0147: CK1: keyed dump caches in the pob-oracle tests (closes CCR-0146 §4)

Unit: **CK1** (hygiene; test-only), then **CK2**. Status: **applied**: §2 by CK1, §3 by CK2
(§5). The `testkit` change of §5.1 (frozen contract, contracts.md: `testkit`, ORC-X) is the
change §3 item 1 proposed; no other contract changes. The number was reserved with
`scripts/port/next_ccr.sh`.

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

## 3. Found by the sweep, not changed by CK1 (applied by CK2, §5)

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

## 5. CK2: §3 applied

### 5.1 `testkit` (item 1)

The sweep's count was low. Every pre-SelectionCache dump directory is served by none of the
current keys, and the current patched-source shas hold **no** dumps in that layout (on
2026-11-12, the 28 shas with `<sha>/<build-key>/<hit>.json.zst` files are all from 2026-10-09
to 11-11). So every caller of `dump_dirs` was reading `.parity/probe/` (a 2026-10-09 corpus run)
or the dumps of a retired sha. Restricting the arm to the current sha would have left the callers
with no dumps at all, and a sha-only key still misses the selection and the probe code. The
callers therefore probe their own selection, as §3 item 1 proposed:

- `testkit::dump_dirs` is removed. `testkit::probe_dumps(bin, xml, stages, opts)` returns a
  `BuildDumps`: one directory, the `state/` of `probe_selection`'s `SelectionCache` entry (keyed
  by the patched-source sha, the build's BLAKE3, the selection, the dump cap, the provenance flag
  and the probe's Rust code; probed in a fresh `bin probe` process when absent;
  `POB_QUERY_CACHE` applies). `$POB_PROBE_DIR/<stem>/state/` (`testkit::override_dir`) is an
  explicit override that is served instead. The `.parity/probe` arm is dropped (opt in with
  `POB_PROBE_DIR=.parity/probe`).
- `testkit::load_dump(dirs, hit)` and `testkit::segment_diff(build, dirs, in, out)` take the
  directories instead of finding them. Helpers: `testkit::default_stages()`
  (`docs/port/stages.toml`), `testkit::selection(hits)` (the `pob_oracle probe` defaults:
  `--max-dumps 200`, provenance on, i.e. what `oracle.sh run --stage <hits>` ran).
- Each caller's selection is the fill command its module docs named, under the stages file named
  there. Because each entry is one probe process, an `:in`/`:out` pair can no longer mix runs.

| test | selection (stages) |
|---|---|
| PF-D | `perform:1943@main,perform:2308@main,pfd:*@main,getCachedOutputValue:*@main` (`corpus/PF-D`) |
| PF-E | `perform:2308@main,perform:2997@main` |
| PF-F | `perform:{1691,1862,2997,3118,3125,3484,3497}@main` (`corpus/PF-F`) |
| OF-A | `offence:362,offence:1037,runSkillFunc:in,runSkillFunc:out`, max 3000 (in each build's child process) |
| OF-D | its own `probe_selection` already; the `dump_dirs` fallback is now `override_dir` |
| OF-E, OF-F | `offence:2607*,offence:3346*,offence:3957*,offence:4155*` (one shared entry) |
| DF-A | `defence:in@main,defence:1211@main` |
| DF-C | `defence:1211@main,defence:end@main` |
| DF-D | `buildDefenceEstimations:1681,buildDefenceEstimations:2153` (item 3) |
| DF-F | `buildDefenceEstimations:2612,buildDefenceEstimations:3165` |
| DF-G | `buildDefenceEstimations:3165@main,buildDefenceEstimations:end@main` |
| ORC-X | `initEnv:out@main,perform:in@main,offence:in@main,offence:out@main` (the corpus run's hits) |

Stages are `docs/port/stages.toml` unless noted. Three more readers of unkeyed dumps, which the
§3 grep pattern missed, were moved the same way:

- **DF-B** `cache_dir` read `<sha>/<build-key>/` of whichever sha held `defence:665` dumps most
  recently. It now uses `defence:659,665,666,674,675,676,1211` (`corpus/DF-B`).
- **AS** `as_l4_corpus` read `.parity/probe/<stem>/state/` directly. It now uses
  `buildActiveSkillModList:in@main,buildActiveSkillModList:out@main`. A failed probe fails the
  test. The exact `AS_L4_PAIRS` count is checked only on the full corpus, the quick-corpus
  policy.
- **ORC-X**'s round trip read the `runSkillFunc` dumps only when the replay test had already
  probed them, and never under `refresh` (`SelectionCache::is_filled`), so their coverage
  depended on test order (a cold run had 0/57). It now probes them itself (`rsf_dumps`), and a
  missing `runSkillFunc` dump is checked against `ORCX_RSF_MISSING` on every build.

DF-A's stage `defence:recovery:in` (P3's 2026-10 stages: before CalcDefence.lua:1211, the
`-- Recovery modifiers` comment) no longer exists. Its successor is `defence:1211` (before 1212,
the first statement after the comment), which records the same state. OF-E's module docs say
"every pass of every `calcs.offence` call the run dumped". It shares OF-F's every-hit selection,
so it is stricter than before (1193 hits). `corpus/PF-D/probe_all.sh` filled the old layout for
PF-D and only that, so it is deleted.

### 5.2 CS3 (item 2)

`ensure_dumps` is an entry of `testkit::oracle_dir`. The source key is that of CS3's patched
tree: its canonical path holds the model hash and the patcher sha (stages, patcher, dump/probe
Lua), and its stat fingerprint covers the edited CalcSetup.lua. The other key parts are the sha,
`model.lua`, the probe flags, `runner.rs`, `bin/pob_oracle.rs` and the `cs3-probe` span of
CS3.rs. The dumps are stored zstd-compressed.

The sweep found a second bug: the patched tree itself was shared. `pob_oracle patch --root`
publishes into the shared `<.parity>/patched-src/<sha>/` whatever root it is given (FUP1), so
CS3 edited that shared copy in place (`0b46a545a10e05d1` holds the `cs3Model` edit), and a
changed `model.lua` was never applied once the edit was there. CS3 now calls
`patcher::patch_into` with its own root `$CARGO_TARGET_TMPDIR/CS3/patched-<BLAKE3(model.lua)>/`
and edits that copy atomically. Nothing else uses the stale shared copy (`corpus/CS3/stages.toml`
is CS3's alone), and the patcher prunes it after 24 h unused.

Red once fresh: CS3 L5 failed 81 of 171 runs. Every mismatch was a JewelFunc closure. FUP3's
`pob-state/2` writes a ModParser closure as `{"$fn": …, "$up": […]}`, and the L5 classifier's
`is_lua_fn` accepted only a one-key `$fn` marker, which the `/1` dumps of the old cache had. The
cause was a harness gap, fixed in CS3.rs: `$up` is an injection input that no comparison reads
(`schema::INPUT_KEYS`), so the class now accepts it. After the fix: 171/171.

### 5.3 DF-D, OF-B (items 3, 4)

DF-D: `target/tmp/dfd-probe` is gone, and the selection is the one in the table above. OF-B:
`.parity/unit/OF-B` is no longer read by default. The selection is `SELECT`
(`offence:1037#*,offence:1692#*`, max 1 000 000, no provenance, i.e. `fill.sh`'s), and
`hit_pairs` reads the entry's `hits.json.zst`. `$OFB_DUMP_DIR/<stem>/` (a `fill.sh` run
directory) stays an explicit override.

### 5.4 Red once fresh: AS

`as_l4_corpus` failed one pair on fresh dumps: allflame_exp_025's buff `modList` had
ShockImmune and SpellBlockChance in the other order. PoB fills it in `pairs(stats)` order over the
string-keyed stat table (CalcActiveSkill.lua:62, then the GlobalEffect split at 805-860), which
is PoB's per-process order (calc doc §3.4/§9.5). The stale corpus dumps happened to agree with
ours. The cause was a harness gap: the test used plain `diff_against`. It now uses FUP1's shared
`diff_against_normalised`, which turns a buff mod list that holds exactly the same mods in
another order into one LUA-ORDER mismatch. That mismatch is counted as explained, and any other
difference still fails.

### 5.5 `POB_QUICK_CORPUS=1` alone

`pob_testpaths::parity_dir` and `pob_oracle::oracle::find_parity_dir` now resolve the nearest
`.parity-quick` when `POB_QUICK_CORPUS=1` is set without `$POB_PARITY_DIR`. `$POB_PARITY_DIR`
still wins, and `cargo.sh` sets both in the main checkout. Before, the variable set alone kept
the 57-build corpus while the tests applied the quick-corpus policy (skipped coverage counts).
The quick-corpus runs below set only `POB_QUICK_CORPUS=1` and got the 14 builds.

### 5.6 Not changed

- P3 reads `.parity/probe/<stem>/` (`hits.json`, `provenance.json`). That is the acceptance test
  of `oracle.sh corpus` itself, which reads what that command wrote, not a dump cache.
- `oracle.sh run` / `oracle.sh corpus` / `bin probe --cache` still write the old
  `<sha>/<build-key>/` layout and `.parity/probe`, for manual use (README). No test reads them.
- OF-F's process peaked at 3.5 GB RSS on the full corpus (`dumps()` parses every OF-F dump of a
  build at once; with one every-hit entry per build there are up to ~100 per build). This is above
  the ~3 GB guideline and under the 5 GB cap. It is not changed here.
- `docs/port/testing.md` §"Converted tests" still names `testkit::dump_dirs`. That file is not
  this unit's.

### 5.7 Results

Every touched test was run one binary at a time (`POB_PORT_TEST_JOBS=1`) with
`POB_ORACLE_REQUIRE=1` under `POB_QUERY_CACHE=refresh`, so every entry was re-made by this
harness. The three tests that went red were fixed (§5.2, §5.4, and ORC-X's coverage, §5.1) and
re-run under `on`. Those runs read the entries the refresh run had just made: the fixes are
outside the entries' keyed code.

Full corpus (`POB_FULL_SUITE=1`, `.parity`, 57 builds):

- PF-D: all57 57/57, unit 1/1 (287 + 6 callee calls replayed). PF-E: all57 57/57, curated
  24/24. PF-F: 58 builds pass, 0 blocked/fail/missing.
- DF-A: resistances 57 actors, 2793 values (= `DFA_MIN_RESIST_VALUES`), 0 differ;
  defenceForConditionals 403 slot outputs, 0 differ. DF-B: 1158/1158 calls (78 minion; unit
  36/36) over 59 builds. DF-C: all57 57/57, curated 14/14, unit 3/3. DF-D: all57 363/363 hits,
  slice5 30/30, unit 198/198. DF-F: all57 57/57, unit 14/14. DF-G: slice5+unit 7/7, all57
  57/57, no-breakdown 64/64.
- OF-A: slice5 30/30, all57 1118/1118 (11 early returns, 7 hook calls replayed). OF-B: 954
  distinct hits exact over 62 builds, 0 unpaired/capped/without dumps. OF-D: all57 1107/1107,
  unit 344/344. OF-E: 1193/1193 hits over 68 builds. OF-F: passHit 1169/1169, combinePasses
  1149/1149.
- ORC-X: round trip initEnv:out 57/57, perform:in 57/57, offence:in 56/56, runSkillFunc:in
  53/53 and :out 53/53 (missing exactly the listed builds); replay offence 56/56, runSkillFunc
  53/53; 6/6 tests passed. On the cold run the round trip found 0/57 runSkillFunc dumps (§5.1).
- AS: as_l4_corpus 56/56 = `AS_L4_PAIRS` (1 LUA-ORDER buff list explained); 4/4 tests passed.
  On the cold run 55/56 (§5.4).
- CS3: L4 337 pass + 6 restarts, 0 fail; L5 171/171. On the cold run L5 was 90/171 (§5.2).

Quick corpus (`POB_QUICK_CORPUS=1` alone, no `POB_PARITY_DIR`: `.parity-quick`, 14 builds, all
under `refresh`):

- PF-D 14/14 + 1/1; PF-E 14/14 + curated 6/6; PF-F 15/15.
- DF-A 14 actors, 686 values, 0 differ; DF-B 322/322 (unit 36/36); DF-C 14/14, 5/5, 3/3;
  DF-D 95/95, slice5 30/30, unit 198/198; DF-F 14/14, 14/14; DF-G 7/7, 14/14, 21/21.
- OF-A 30/30, 282/282; OF-B 258 distinct hits over 19 builds; OF-D 282/282, 344/344; OF-E
  357/357; OF-F 333/333, 324/324.
- ORC-X: round trip 14/14, 14/14, 13/13, 13/13, 13/13; replay 13/13, 13/13. AS 14/14. CS3 L4
  83/83, L5 42/42.
