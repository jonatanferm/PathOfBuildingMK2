# Calc goldens

PoB's answers, recorded once while PoB still ran under LuaJIT, and the Rust calc checked against
them with no PoB. The design is `docs/standalone/GOLDEN-PLAN.md` (§2 storage, §3 recorder and
runner); this file is the how-to.

## Layout

```
MANIFEST.json                  every file: set, path, BLAKE3, lines, bytes, origin, pins; the builds; caps and sizes
coverage.json                  record-time coverage witnesses (GOLD3)
builds/<corpus>/*.xml          the fixtures (GOLD2): all57, fresh, special/<unit>, derived/<unit>
e2e/<set>/<corpus>/<stem>.jsonl          E2E build goldens, plain, one file per build
fn/<area>/<set>[.<shard>].jsonl[.zst]    function-level query sets
fuzz/<set>.<shard>.jsonl.zst             fuzz output sets
overlay/<base path>.bless.jsonl          deliberate deviations from PoB (bless)
```

A golden file is `golden/1` (GOLDEN-PLAN §3.2): a header line, then lines sorted by key, one value
per line: `[key, value]` (E2E) or `[key, input, value]` (query). Value markers: `$num`, `$anyOf`,
`$unordered`, `$error`, `$host`, `$known`, `$perOrder`, `$absent`. A file whose plain JSONL would
exceed about 4 MB is split by key into `.jsonl.zst` shards; no file may exceed 8 MB on disk.

## Run

```sh
scripts/port/cargo.sh test -p pob-golden --test golden_e2e    # e2e.* sets
scripts/port/cargo.sh test -p pob-golden --test golden_fn     # fn.* sets
scripts/port/cargo.sh test -p pob-golden --test golden_fuzz   # fuzz.* sets
```

Each binary walks MANIFEST.json, so a new golden file needs no new test code. Nothing is skipped:
a missing file, a file whose BLAKE3 differs from MANIFEST, a build XML whose BLAKE3 differs from
the header, a file MANIFEST does not list, or a set without an adapter fails.

| Variable | Effect |
|---|---|
| `POB_GOLDEN_JOBS` | worker threads (default 3) |
| `POB_GOLDEN_FILTER=<set glob>[:<key glob>]` | run only the matching sets; a bless writes only the matching keys |
| `POB_GOLDEN_DIFFS=<file>` | every mismatch as TSV (`set file key path kind pob rust`) |
| `POB_GOLDEN_SELFTEST=1` | the negative control (§4.4): one value per set is changed, and each set must fail exactly once, on that key |
| `POB_GOLDEN_DIR` | another goldens directory |

The report prints one `golden <set>: compared N (pin N), exact …, anyOf …, host …, known …,
regenerated …, bitExact N (pin N), order-only …, failed …` line per set, then the first 20 failures
per file.

## Re-record = bless (§3.6)

There is no PoB any more, so "re-record" means: accept Rust's new value deliberately.

```sh
POB_GOLDEN_BLESS=1 POB_GOLDEN_REASON="<why: the behaviour, and the issue or PoB commit it follows>" \
  [POB_GOLDEN_FILTER=<set>[:<key glob>]] scripts/port/cargo.sh test -p pob-golden --test golden_e2e
```

- One overlay line per **failing** value: `[key, pob_value, new_value, reason, commit]`. The
  PoB-recorded file is never touched. MANIFEST is updated (the overlay's hash, a lowered
  `bitExact` pin).
- It refuses without a reason, or when more than `POB_GOLDEN_BLESS_MAX` (default 200) values
  match. Split a bigger bless by key, or raise the limit on purpose.
- An overlay line whose `pob_value` no longer matches the base file is stale and fails.
- `POB_GOLDEN_BLESS=rebase` (exceptional, e.g. a new game-data snapshot) rewrites the base files
  with Rust's values and drops their overlays; MANIFEST records `origin: rust@<sha>`, and the PR
  says so in its title.

Then run the suite again, and its negative control.

## Review checklist (required when `tests/golden/` changes)

1. Paste the `golden diff origin/rust-port` summary (`scripts/port/cargo.sh run -p pob-golden --bin
   golden -- diff origin/rust-port`). Every changed key is one the change meant to move; no
   unrelated set changed.
2. Each overlay reason names the behaviour, and the issue or PoB commit it follows.
3. No `$anyOf`, `$unordered`, `$host` or `$known` marker was removed or widened, unless the PR
   explains why.
4. The MANIFEST pins changed only by the overlaid or added keys. A lowered `bitExact` pin carries
   its reason.
5. Fixture XML unchanged, or the set re-recorded with it (the BLAKE3 matches).
6. The full golden suite is green after the bless, and its negative control still fails as planned
   (`POB_GOLDEN_SELFTEST=1`).

A PR that changes `crates/pob-golden/src/adapter/` must show `golden diff` = 0, or overlays with
reasons (§3.4).

## Tools

```sh
scripts/port/cargo.sh run -p pob-golden --bin golden -- diff [<git-rev>]       # default HEAD; exit 1 when they differ
scripts/port/cargo.sh run -p pob-golden --bin golden -- diff --old <dir> --new <dir>
scripts/port/cargo.sh run -p pob-golden --bin golden -- manifest check|refresh
scripts/golden/size.sh                     # measure working-tree and packed sizes into MANIFEST
```

The recorder, while PoB exists (GOLDEN-PLAN §3.1):

```sh
scripts/port/cargo.sh run -p pob-oracle --bin pob_golden_record -- <set|tier|all> \
  [--corpus all57|fresh|special|derived] [--builds a,b] [--runs K] [--out tests/golden] [--verify]
```

## Tests moved out of the oracle crates (GOLDEN-PLAN §1.7)

The cut-over's `scripts/golden/check_moved.sh` reads this list: each function must be gone from
`crates/pob-oracle` and `crates/pob-ui-oracle` (and from its old file, for the pob-calc move) and
present in its destination. One row per line: `function | from | to | owner`.

<!-- moved-tests:begin -->
```
xml_reader_writer | crates/pob-oracle/tests/BM-A.rs | crates/pob-calc/tests/model_xml.rs | GOLD5
set_buff_exports | crates/pob-oracle/tests/BM-B.rs | crates/pob-calc/tests/tabs_fresh_build.rs | GOLD5
fresh_build_entry_points | crates/pob-oracle/tests/BM-B.rs | crates/pob-calc/tests/tabs_fresh_build.rs | GOLD5
pwr_coroutine_steps | crates/pob-oracle/tests/PWR.rs | crates/pob-golden/tests/golden_e2e.rs (invariants::power_steps) | GOLD6
pwr_stat_fns | crates/pob-oracle/tests/PWR.rs | crates/pob-calc/tests/power_stats.rs | GOLD6
ofc_combine_stat_modes | crates/pob-oracle/tests/OF-C.rs | crates/pob-calc/tests/combine_stat.rs | GOLD12
compile_zero_unknown_keys | crates/pob-oracle/tests/U0.rs | crates/pob-gamedata/tests/dump_load.rs | GOLD1
unknown_key_is_an_error | crates/pob-oracle/tests/U0.rs | crates/pob-gamedata/tests/dump_load.rs | GOLD1
parse_raw_units | crates/pob-oracle/tests/IT-A.rs | crates/pob-calc/tests/item_units.rs | GOLD9
item_core_units | crates/pob-oracle/tests/IT-B.rs | crates/pob-calc/tests/item_units.rs | GOLD9
item_tools_units | crates/pob-oracle/tests/IT-C.rs | crates/pob-calc/tests/item_units.rs | GOLD9
modcache_tier_serves_cached_lines | crates/pob-oracle/tests/MP0.rs | crates/pob-modparser/tests/mod_cache.rs | GOLD7
appendmod_and_lookups | crates/pob-oracle/tests/MP0.rs | crates/pob-modparser/tests/mod_cache.rs | GOLD7
ct_global_cache | crates/pob-oracle/tests/CT.rs | crates/pob-calc/tests/global_cache.rs | GOLD11
as_copy_actor_keepalive | crates/pob-oracle/tests/AS.rs | crates/pob-calc/tests/copy_actor_keepalive.rs | GOLD11
cs2_l4_overrides | crates/pob-oracle/tests/CS2.rs | e2e.mcq (golden_e2e) | GOLD6
ca_d13_fixed_point | crates/pob-oracle/tests/CA.rs | e2e.d13 (golden_e2e) | GOLD3
ca_level_cost_cleared | crates/pob-oracle/tests/CA.rs | e2e.levelcost (golden_e2e) | GOLD3
ordered_and_bounded | crates/pob-oracle/src/par.rs | crates/pob-golden/src/par.rs | GOLD1
worker_panic_is_reraised_in_order | crates/pob-oracle/src/par.rs | crates/pob-golden/src/par.rs | GOLD1
consumer_panic_stops_workers | crates/pob-oracle/src/par.rs | crates/pob-golden/src/par.rs | GOLD1
bounded_in_flight | crates/pob-oracle/src/par.rs | crates/pob-golden/src/par.rs | GOLD1
release_shared_only_outside_pools | crates/pob-oracle/src/par.rs | crates/pob-golden/src/par.rs | GOLD1
init_per_worker | crates/pob-oracle/src/par.rs | crates/pob-golden/src/par.rs | GOLD1
tolerance | crates/pob-oracle/src/diff.rs | crates/pob-golden/src/compare.rs | GOLD1
stores_multiset_and_order | crates/pob-oracle/src/diff.rs | crates/pob-golden/src/compare.rs | GOLD1
nondet_values_and_orders | crates/pob-oracle/src/diff.rs | crates/pob-golden/src/compare.rs | GOLD1
unordered_lists_with_nested_orders | crates/pob-oracle/src/diff.rs | crates/pob-golden/src/compare.rs | GOLD1
store_alternatives_per_name | crates/pob-oracle/src/diff.rs | crates/pob-golden/src/compare.rs | GOLD1
ctor_pairs_follow_field_order | crates/pob-calc/tests/hyg1_len.rs | crates/pob-calc/tests/ctor_pairs.rs | GOLD1
u53_perf | crates/pob-ui-oracle/tests/u53_perf.rs | crates/pob-app/tests/tree_perf.rs | GOLD13
u26_progress_steps | crates/pob-ui-oracle/tests/ | crates/pob-app/tests/ (or dropped, GOLDEN-GATE.md) | GOLD13
u48_progress_steps | crates/pob-ui-oracle/tests/ | crates/pob-app/tests/ (or dropped, GOLDEN-GATE.md) | GOLD13
u38_undo_round_trip | crates/pob-ui-oracle/tests/ | crates/pob-app/tests/ (or dropped, GOLDEN-GATE.md) | GOLD13
u43_share_rust | crates/pob-ui-oracle/tests/ | crates/pob-app/tests/ (or dropped, GOLDEN-GATE.md) | GOLD13
u44_rust_unit | crates/pob-ui-oracle/tests/ | crates/pob-app/tests/ (or dropped, GOLDEN-GATE.md) | GOLD13
u45_disable_party_effects | crates/pob-ui-oracle/tests/ | crates/pob-app/tests/ (or dropped, GOLDEN-GATE.md) | GOLD13
```
<!-- moved-tests:end -->

`ca_level_cost_cleared` stands for every `ca_level_cost_cleared*` test. Dropped on purpose,
although they need no PoB (§1.7): CT `ct_data_census`, `ct_copy_table_safe`; AS `as_census`; MP0
`generated_list_sizes`; PF-B `pfb_ally_life_pools`; `u31_rebuilt_slot_controls`; CS3
`cs3_object_key_order_class`; PF-G `pfg_replay_hook_roundtrip`; OF-B `ofb_attribution_patterns`;
PF-A `pfa_stages_file_patches`; U0 `registry_complete`, `provenance_check_and_coverage`,
`stages_*`; `u0_smoke`, `worker_smoke`, `oracle_harness`; `src/runner.rs`, `src/testkit.rs`;
pob-ui-oracle `u29_data_invariants` and its src tests; diff.rs `units`. They go with their crates.
