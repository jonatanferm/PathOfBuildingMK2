# CCR-0146: MI1: the MI L4 test grafts `mirage.output` (CCR-0090 §4.3 superseded), and probes through the content-keyed selection cache

Unit: **MI1** (hygiene; test-only, `crates/pob-oracle/tests/MI.rs`). Status: **applied** by MI1
(no frozen contract changes; this records that CCR-0090 §4.3's description of the test no longer
holds). Number reserved with `scripts/port/next_ccr.sh`.

## 1. Problem

`mi_l4_mirages_in_to_out` failed on the full corpus (`POB_FULL_SUITE=1`) with 13 divergent calls
of 74, every one the Mirage Archer family, each with exactly one final mismatch:

```
env.player.mainSkill.mirage.output Value pob={"AbsorptionCharges":0,...} ours={"$foreign":{"env":"env8","path":"player.output"}}
```

Calls: allflame_exp_013 in#19/#21/#4@main (Kinetic Blast), allflame_exp_036 in#22/#24/#4@main
(Toxic Rain), allflame_exp_051 in#23/#25/#4@main (Lightning Arrow),
upstream_Mirage_Archer_Toxic_Rain in#16/#19/#21/#4@main (Toxic Rain).

Cause: harness, not the port and not PoB nondeterminism. CCR-0090 §4.3 (MI, 2026-11-10 01:02)
kept `newEnv.player.output` as PoB's `$foreign` marker for Mirage Archer / Sacred Wisps, because
the dumps of the time wrote another env's output as a marker. ORC-X2 (7031a8f, merged 2026-11-10
09:40) changed `crates/pob-oracle/lua/state_dump.lua` (`inlineForeign`, ~line 384): another env's
actors, mod stores and `*.output` tables are written **in full** where a dump reaches them. Since
then `mirages:out` holds `env.player.mainSkill.mirage.output` (CalcMirages.lua:100, 326) as
the full table, and the test's placeholder (`mark_output_foreign`) can no longer match it.

The failure stayed hidden because the test kept its dumps in `target/tmp/mi-probe/<stem>/` and
reused them whenever a `hits.json` was there, with no key at all. The full-suite worktree
(`../pob-fullsuite`, a persistent target dir) still had the dumps made 2026-11-10 04:12, before
ORC-X2, so the 2026-11-12 07:27 full suite (f571987) passed MI in 36 s on dumps of an older
encoder. Fresh worktrees probe again (~400-670 s) and failed. Changing the harness, the dump Lua,
the PoB tree, `corpus/MI/stages.toml` or the build XML did not invalidate those dumps.

## 2. Change (MI.rs only)

1. **Probe cache.** `probe_dumps` uses `testkit::probe_selection` (HYG3/TP1
   `runner::SelectionCache`, `.parity/probe-cache/<sha>/<build-key>/sel-<key>/`), like CS2 /
   DF-E / OF-C / OF-D. `<sha>` is `patcher::plan_sha` (patcher version, `state_dump.lua`,
   `provenance.lua`, `probe.lua`, the stages, the patched files, the Lua runtime versions, the
   PoB tree's stat fingerprint), `<build-key>` the build's BLAKE3, `sel-<key>` the selection, the
   dump cap and the Rust code of the probe run (`runner.rs`, `bin/pob_oracle.rs`, `jit.rs`,
   `headless.rs`, `oracle.rs`, `oracle.lua`). Changing the harness or the dump format now
   probes again. `POB_QUERY_CACHE=off` / `verify` work as for the other users. `mirage_calls`
   reads `hits.json(.zst)` and the dump files with or without `.zst`.
2. **Mirage Archer / Sacred Wisps graft.** The perform replay grafts PoB's
   `mirages:out` `env.player.mainSkill.mirage.output` (its full occurrence, a `$ref` followed)
   into newEnv's `player.output`, as it already did for `env.player.output` for Reflection /
   Tawhoa's Chosen (`graft_output` now takes the subtree's base path). A `$foreign` marker there
   (a dump of the pre-ORC-X2 encoder) keeps the old placeholder.

What the gate checks: the output's **content** is PoB's by construction (as for Reflection,
CCR-0090 §4.3), but MI's assignment is still checked. A mutation that stores a fresh table at
CalcMirages.lua:100 (`mirage.set("output", Table::new())`) makes allflame_exp_036's 3 calls
divergent with 621 mismatches each. `mirage.minion.output` (lines 104, 330): no corpus call
has a mirage minion, so it is not grafted. If a build with one is added, it will show up as a
divergent mismatch, not pass silently.

## 3. Results

- `POB_FULL_SUITE=1`: 57 builds, 74 calls, 74 clean, 0 replay-limited, 0 divergent, 0 failed
  (42.8 s from the cache; 665 s to fill it).
- Quick corpus (`.parity-quick`, 14 builds): 21 calls, 21 clean.
- `POB_QUERY_CACHE=off` (fresh PoB runs, private dirs), allflame_exp_036, allflame_exp_050 and
  upstream_Dual_Savior: 10 calls, 10 clean.

## 4. Not done here (other owners)

SK-B.rs:131, TR-A.rs:960 and TR-B.rs:208 have the same keyless `target/tmp/*/hits.json`
pattern. CA.rs:771 and PF-A.rs keep private dirs keyed only by `format.txt` (STATE_FORMAT),
which does not cover `inlineForeign`-style encoder changes that keep the format string. They can
serve dumps from an older harness in a long-lived target dir (the full-suite worktree) in the
same way.
