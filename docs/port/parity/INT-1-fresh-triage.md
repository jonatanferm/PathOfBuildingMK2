# INT-1 fresh-corpus triage (INT-1-FRESH)

Unit INT-1-FRESH (calc doc §11.1 row INT-1, the "100 fresh poe.ninja builds" half of the gate),
2026-11-12. This is a measurement and a diagnosis only. No product code changed. `pob-calc` is
untouched, because CSPLIT is moving its files. The fixes are grouped into the fix units of §6.
The corpus is described in `fresh-corpus.md`. The all57 half of the gate is done
(`INT-1-triage.md` §8).

## 1. Result (measured)

The run used rust-port `645e2e1` plus this unit's harness change (`CA.rs`: `CA_CORPUS_DIR`,
`CA_L6_DIFFS`). It was started with `scripts/port/fresh_corpus_run.sh`: one
`scripts/port/cargo.sh test --release -p pob-oracle --test CA -- ca_l6_corpus ca_d13_fixed_point`
per chunk of up to 20 builds, one chunk at a time. Inside a chunk the harness takes the builds
one at a time and drops each build's state. The test binary's peak RSS, sampled every 3 s, was
1.73 GB. The PoB runs are new: PoB v2.67.2 under LuaJIT, through the harness's `pob_run`.

| metric | measured |
|---|---|
| builds | **100** (0 PoB runs failed) |
| Rust errors (any stage: load, MAIN, CALCS, getMiscCalculator, configTab, misc call) | **0** |
| headline keys exact | **1,300 / 1,300**; 100 of 100 builds have every headline key exact |
| all main-output keys exact | **85,603 / 85,606** (99.996 %). All 85,603 PoB keys are exact; the 3 not exact are Rust-only keys |
| CALCS lists (Buff/Combat/CurseList) | **300 / 300** |
| used-tracking key sets | **900 / 900** |
| misc calculator values | **2,200 / 2,200** |
| D13 fixed point (MAIN and CALCS) | **99 / 100**; 0 runs raised; fresh_017 differs, which is PoB behaviour (§4 B) |

The per-chunk summary lines (`results/<first>..<last>.log` under `.parity/fresh/`) add up to
these totals:

```
CA L6: 20+20+20+2+20+16+2 = 100 builds (0 PoB runs failed), 0 Rust runs raised; headline 1300/1300,
100 builds with every headline key exact; all keys 85603/85606; CALCS lists 300/300;
used-tracking key sets 900/900; misc calculator 2200/2200
CA D13: 100 builds, 100 ran, 99 fixed points, 1 differ, 0 Lua errors
```

The per-build table is in §7. `scripts/port/fresh_corpus_summary.py --families` prints it and the
totals from the result files.

## 2. Method

1. **Corpus.** 105 poe.ninja characters were fetched and 100 kept (`fresh-corpus.md`). The 5 drops
   are all fetch failures: the character API answered 404, and so did up to 3 fall-back rows. No
   build was dropped for a PoB error or for newer game data. The fetched data needed nothing
   newer than PoB 2.67.2 ships: the 3.29 and 3.28 trees, all nodes, gems and item bases are
   known. The check, `scripts/port/fresh_corpus_datacheck.lua`, ran inside PoB after each load.
2. **Harness.** `CA_CORPUS_DIR` points `corpus()` at `.parity/fresh`. In that mode D13 skips
   slice5, and the all57 total pins do not apply. `CA_L6_DIFFS=<file>` writes every L6 value that
   is not exact as a TSV line `stem block key rust pob`. The blocks are main, calcs, used,
   misc.callDefault, misc.callNoFullDPS and error. These are the only harness changes.
   What is compared and gated did not change. So the exact gates fail where a value is not
   exact: the first chunk (fresh_000..019, which holds fresh_015, _017 and _018) exited 101 on
   §4 A and §4 B, and every other chunk passed.
3. **First diverging stage.** For each bucket, PoB's state was read directly. A Lua snippet ran in
   `pob_headless` (POB_ENGINE=jit) after the load, and it read the cached env of the skill or
   compared warm and cold runs. The Rust side was checked with a local, env-gated candidate fix
   (§5), which was not committed. Following INT-1-triage.md, a bucket counts as diagnosed only
   when its candidate fix, applied alone, removes exactly that bucket's mismatches.
4. **Not covered here.**
   - Breakdowns are INT-3, and L6 does not compare them.
   - PoB nondeterminism: `oracle.sh nondet` was not run on the fresh corpus. No mismatch
     needs it, because every PoB value is matched. Later, fresh_076 `ManaCostRaw` turned out
     to be nondeterministic in PoB. FUP4 recorded it with `oracle.sh nondet --fresh` and
     `ca_l6_corpus` now reads the record (FOLLOWUPS "FUP2" item 1).
   - The ±0 tolerance (CCR-0001/0002): how many keys pass only through it was not counted on
     this corpus.

## 3. Buckets, ordered by impact

No Rust panic or Rust error occurred on any build, so this list has no error bucket.

| # | bucket | builds | values affected | first diverging stage | owner unit / Rust file | Lua evidence | proposed fix | est. size |
|---|---|---|---|---|---|---|---|---|
| A | A triggered item-granted skill (an item's "Curse Enemies with X on Hit") still has its gem cost in Rust. PoB has emptied that level's cost table. So Rust adds a mana-cost warning that PoB does not have | fresh_015 (Vulnerability, rare ring "Plague Eye"), fresh_018 (Vulnerability + Temporal Chains, Asenath's Gentle Touch) | 3 Rust-only main keys: `ManaCostWarningList.1` (×2) and `.2` | CalcOffence.lua:1720 (OF-C `seg_costsAndPasses`) in the cached env of the curse skill. PoB reads `grantedEffectLevel.cost = {}`, so `ManaCost` is 0. Rust reads the data's `cost.Mana = 16` (Rust's cached `ManaCost` was not dumped; the candidate fix of §5 confirms the path). The warning loop then fires at Calcs.lua:410-420 (ManaUnreserved 5 and 1) | **OF-C**: `calc_offence/costs_passes.rs` (L1720 block, `costs_passes.rs:289-305`). The contract is CCR-0019 §8, which BM-B implements (`model/skills_tab.rs:141-155`, `levelCostCleared`) and OF-C never reads | SkillsTab.lua:1188-1191: `if gemInstance.triggered then … gemInstance.grantedEffect.levels[gemInstance.level].cost = {}`, a write into PoB's global `data`. CalcSetup.lua:1500 `activeGemInstance.triggered = grantedSkill.triggered`. ModParser.lua:3383 / 2063 `triggerExtraSkill(…)` sets `triggered = true`. CalcOffence.lua:1720 `… or activeSkill.activeEffect.grantedEffectLevel.cost and …grantedEffectLevel.cost[resource] or nil` | At L1720, treat the cost as `{}` when `skills_tab::levelCostCleared(grantedEffect.id, level)` holds, as CCR-0019 §8 already requires. Reset the cleared set per build in the CA harness (§4 A, "process scope") | XS |
| B | D13 on a warm GlobalCache: the second `buildOutput` differs from the first. PoB does the same | fresh_017 (SSF Allflame Champion, Lacerate of Butchering, bleed) | 0 L6 values. D13 reports 49 differences | PoB itself: wipe, then `buildOutput(MAIN)` twice gives 24 of 78,992 output keys that differ (BleedStacks 0.675 → 1.25, LifeLeechInstances 2.7 → 5, MainHand.TotalDPS 2,277,409.28 → 4,217,424.59); with a wipe between the runs, 0 keys differ. Rust's warm run has the same values | **CA** (test): `crates/pob-oracle/tests/CA.rs` `D13_POB_WARM_CACHE` | The same class as FOLLOWUPS "CA" 5 (allflame_exp_050, also a bleed build): a GlobalCache entry left by the first run feeds the second run's bleed and leech stacks | Add fresh_017 to `D13_POB_WARM_CACHE` with PoB's warm values (§4 B). Then the fixed point is checked after a wipe, as for allflame_exp_050 | XS |

**Totals.** A explains all 3 L6 values that are not exact (85,606 − 85,603), and B explains the
only D13 failure. Every PoB value of the 100 builds (1,300 headline values, 85,603 main-output
keys, 300 CALCS lists, 900 used-tracking sets, 2,200 misc values) is already exact.

## 4. Bucket details

### A. Triggered item skills keep their gem cost (OF-C; fresh_015, fresh_018)

**Inputs.**
- fresh_015 is a Standard Champion (Reap and Corrupting Fever, Lifetap; ManaUnreserved 5).
  - Ring 2 is the rare "Plague Eye, Helical Ring" with the mod `Curse Enemies with
    Vulnerability on Hit`.
  - PoB builds a socket group with `source = "Item:29:Plague Eye, Helical Ring"`, holding a
    level-1 `Vulnerability`.
- fresh_018 is a Mirage Champion (Corrupting Fever). Its Asenath's Gentle Touch grants
  Vulnerability and Temporal Chains the same way. ManaUnreserved is 1.

**PoB state** (pob_headless, the cached MAIN env of `Vulnerability_RING2_1_10`):

```
ManaCost=0 ManaCostRaw=0 ManaHasCost=false HasNoCost=nil BaseManaCostOverride=nil
grantedEffectLevel.cost.Mana=nil  INC ManaCost=0  More=1  srcInstance.triggered=true
```

- `Data/Skills/act_str.lua:12958` gives Vulnerability level 1 the cost `{ Mana = 16 }`.
- `grantedEffectLevel.cost` is empty in PoB, because SkillsTab.lua:1188-1191 replaced it.
  `ProcessSocketGroup` does this for every gem instance that is specified by skill id and is
  `triggered`, meaning item-granted triggered skills.
- So CalcOffence.lua:1720 finds no `skillCost`, and the curse's `ManaCost` is 0. PoB's `costs`
  dump (`ca.json`) has `ManaCost` 0 for `Vulnerability_RING2_1_10` too.
- The cost-warning loop at Calcs.lua:405-420 compares that cached cost with the main env's
  `ManaUnreserved`. PoB adds no warning.

**Rust.**
- `model/skills_tab.rs:1029-1041` ports SkillsTab.lua:1188-1191. It records
  `(grantedEffect.id, level)` in `LEVEL_COST_CLEARED`, a thread-local set, because
  `pob_gamedata::Data` is immutable (CCR-0019 §8).
- CCR-0019 §8 obliges "every reader of `grantedEffectLevel.cost` (… OF-C …)" to treat the cost
  as `{}` afterwards.
- `calc_offence/costs_passes.rs:289-305` (the L1720 block) reads `gel.cost` directly and never
  calls `levelCostCleared`. The cached curse env therefore gets a mana cost (16 before modifiers; Rust's value was not
  dumped), and
  `calcs.rs` (Calcs.lua:400-435) appends the skill name to `ManaCostWarningList`. These keys are
  Rust-only: PoB has no `ManaCostWarningList` at all.
- The other Lua reader of a level's `cost` is the gem tooltip (GemTooltip.lua:81), and the UI
  side already follows CCR-0019 §8 (`pob-app/src/controls/gem_tooltip.rs:199`).

**Proposed fix (OF-C).**
- In the L1720 lookup, check the active effect: if
  `skills_tab::levelCostCleared(activeEffect.grantedEffect.id, activeEffect.level)` holds, the
  lookup yields `nil` (an empty `cost` table).
- **Which level is keyed.** Lua mutates the table `levels[gemInstance.level]` and reads the
  table `activeEffect.grantedEffectLevel`. The two are the same table whenever the active
  effect's level picks that entry. The fix unit must confirm which level CalcActiveSkill /
  `calcLib.validateGemLevel` use to choose `grantedEffectLevel`, and key the check with that
  level. On both builds it is 1.

**Process scope (harness).**
- In PoB the emptied table stays for the rest of the process.
- The oracle boots a fresh PoB for every build (`pob_run` → `JitPob::boot_oracle`).
- The CA test runs many builds in one thread, and `LEVEL_COST_CLEARED` is never reset.
- Once OF-C reads the set, a build that runs after fresh_015 in the same test process would see
  Vulnerability level 1 as free even when it is cast normally. Its oracle run would not.
- The fix therefore needs a per-build reset in the harness. `CA.rs` already calls
  `pob_calc::global_cache::clear_all()` and `lut::clear_timeless_jewel_luts` per build. It needs
  a `skills_tab` reset function, which is a new `pub fn` in BM-B's `model/skills_tab.rs` and so a
  CCR amending CCR-0019 §8. The desktop app keeps PoB's process scope.

**Experiment.** §5 has the corpus-wide numbers of the candidate fix.

### B. PoB is not a fixed point on a warm GlobalCache (CA harness; fresh_017)

- fresh_017 is an SSF Allflame Champion: Lacerate of Butchering, a bleed build with Foulborn
  Uul-Netol's Kiss.
- `ca_d13_fixed_point` reports 49 differences. The first 10 are BleedDPS, BleedDamage,
  BleedRollAverage, BleedStackPotential, BleedStacks, CombinedAvg, CombinedDPS,
  LifeLeechGainRate, LifeLeechInstances and LifeLeechRate. Rust's warm run gives
  BleedDPS 53,560.42871176573, BleedStacks 1.25, LifeLeechInstances 5 and CombinedAvg
  4,217,424.593125107.
- PoB was measured with `pob_headless` (POB_ENGINE=jit) after the load: `wipeGlobalCache()`,
  `calcs.buildOutput(build, "MAIN")` twice, then a wipe and a third run. Of 78,992 flattened
  numeric keys, 24 differ between the first two runs, and 0 differ between the first and the
  third run:
  - MainHand.BleedDPS 34,158.45026412001 → 53,560.428711766;
  - BleedStacks 0.675 → 1.25;
  - LifeLeechInstances and ManaLeechInstances 2.7 → 5;
  - MainHand.BleedDotMulti 1.19275 → 1.2129666306025;
  - CombinedAvg and MainHand.TotalDPS 2,277,409.2802875577 → 4,217,424.5931251;
  - ManaLeechRate 38.34 → 71.
- These are Rust's warm values to the printed precision. L6 compares the first, cold run, and
  it is exact for this build (779/779 keys).
- So this is PoB behaviour that Rust reproduces, the same class as FOLLOWUPS "CA" 5
  (allflame_exp_050: BleedStacks 0.5875 → 1.25, LifeLeechInstances 2.35 → 5). It is not a
  calc bug.
- Proposed (CA): add `("fresh_017", &[("BleedStacks", 1.25), ("LifeLeechInstances", 5.0),
  ("BleedDPS", 53_560.428_711_766), ...])` to `D13_POB_WARM_CACHE`. The fix unit takes the
  exact `TotalDPS` / `CombinedDPS` values from a PoB warm run, as FOLLOWUPS "CA" 5 did. The
  warm run is then checked against PoB's values, and the fixed point is checked after a wipe.
- The underlying PoB behaviour (which cache entry feeds the stack counts) is not diagnosed
  further. Neither implementation needs a change.

## 5. Candidate fix A on the whole corpus (local experiment)

The candidate fix was a local edit of `costs_passes.rs` behind an env var. It was not
committed and was reverted afterwards. It made the L1720 lookup yield `nil` when
`skills_tab::levelCostCleared(activeEffect.grantedEffect.id, activeEffect.level)` holds. The
whole fresh corpus then ran in one test process (`INT1F_EXP_A=1 CA_CORPUS_DIR=.parity/fresh
… ca_l6_corpus`, PoB runs from the cache; peak RSS 2.41 GB):

```
CA L6: 100 builds (0 PoB runs failed), 0 Rust runs raised; headline 1300/1300 exact (100.0%),
100 builds with every headline key exact; all keys 85603/85603 exact (100.00%); CALCS lists
300/300; used-tracking key sets 900/900; misc calculator 2200/2200
test result: ok. 1 passed
```

- The 3 Rust-only `ManaCostWarningList` keys are gone, and no other value changed:
  `CA_L6_DIFFS` wrote no line.
- On this corpus the missing per-build reset of `LEVEL_COST_CLEARED` (§4 A, "process scope")
  shows nothing. Six builds have an item-granted level-1 curse, which is the case that clears
  a level: fresh_000, _015, _016, _017 and _018 have Vulnerability, and fresh_018 and _078 have
  Temporal Chains. In the four builds other than fresh_015 and _018, the cost changes no
  warning, because enough mana is unreserved. Every self-cast
  Vulnerability or Temporal Chains in the corpus has a level other than 1 (5 to 21; fresh_010,
  _011, _012, _036, _039, _074, _076, _098). So the reset is argued from the code, not measured,
  and INT-1F-A's unit test must cover it.
- Not run with the candidate fix: the all57 corpus, whose PoB runs are not cached in this
  worktree, and OF-C's L4 suite. INT-1F-A must run both.

## 6. Fix units

The two units have disjoint files and can run in parallel. Both owner paths are pre-CSPLIT. After
CSPLIT, the fix lands wherever `costs_passes.rs` and `skills_tab.rs` have moved.

| unit | buckets | owned files (only these) | acceptance | size |
|---|---|---|---|---|
| **INT-1F-A** (owner OF-C, with BM-B for the reset) | A | `pob-calc` `calc_offence/costs_passes.rs` (the L1720 cost lookup reads `skills_tab::levelCostCleared`, CCR-0019 §8). `pob-calc` `model/skills_tab.rs`: a `pub fn` that clears `LEVEL_COST_CLEARED`, via a new CCR from `scripts/port/next_ccr.sh` amending CCR-0019 §8 (per-process state, reset per build by the oracle harness). `crates/pob-oracle/tests/CA.rs`: call the reset next to `global_cache::clear_all()` per build | `CA_CORPUS_DIR=.parity/fresh` `ca_l6_corpus`: 0 Rust errors, 1,300/1,300 headline, **85,603/85,603** keys (the 3 Rust-only `ManaCostWarningList` keys gone, nothing else changed), 300/300 CALCS lists, 900/900 used-tracking, 2,200/2,200 misc. all57 `ca_l6_corpus` unchanged at its pinned totals. OF-C's L4 suite unchanged. A unit test: after `UpdateSocketGroups` on a build with an item-granted triggered curse, the curse's cached `ManaCost` is 0, and after the reset a normal level-1 gem of the same skill has its cost again | XS |
| **INT-1F-B** (owner CA) | B + gates | `crates/pob-oracle/tests/CA.rs`: fresh_017 in `D13_POB_WARM_CACHE`, with values measured in PoB. Optionally pin the fresh corpus's totals under `CA_CORPUS_DIR` (100 builds, 1,300 headline, 85,603 keys, 300 / 900 / 2,200) like the all57 pins. `docs/port/FOLLOWUPS.md` "CA" 5 notes the second build | `CA_CORPUS_DIR=.parity/fresh` `ca_d13_fixed_point`: 100 ran, 0 differ, 0 errors, 1 warm-cache run checked (fresh_017). The all57 D13 is unchanged | XS |

With both units merged, both halves of INT-1's gate are met: L6 exact on all57 and on 100 fresh
poe.ninja builds.

## 7. Per-build results (baseline, rust-port 645e2e1 + harness change)

`keys exact / compared` counts PoB's main-output keys plus the Rust-only keys, as
`ca_l6_corpus` does. Each build has 3 CALCS lists, 9 used-tracking key sets and 22 misc values;
the columns count the ones that are not exact.

| stem | headline exact | keys exact / compared | CALCS lists not exact | used-tracking sets differing | misc values not exact | Rust errors | D13 |
|---|---|---|---|---|---|---|---|
| fresh_000 | 13/13 | 831/831 | 0 | 0 | 0 | 0 | fixed point |
| fresh_001 | 13/13 | 746/746 | 0 | 0 | 0 | 0 | fixed point |
| fresh_002 | 13/13 | 779/779 | 0 | 0 | 0 | 0 | fixed point |
| fresh_003 | 13/13 | 1446/1446 | 0 | 0 | 0 | 0 | fixed point |
| fresh_004 | 13/13 | 792/792 | 0 | 0 | 0 | 0 | fixed point |
| fresh_005 | 13/13 | 766/766 | 0 | 0 | 0 | 0 | fixed point |
| fresh_006 | 13/13 | 776/776 | 0 | 0 | 0 | 0 | fixed point |
| fresh_007 | 13/13 | 837/837 | 0 | 0 | 0 | 0 | fixed point |
| fresh_008 | 13/13 | 770/770 | 0 | 0 | 0 | 0 | fixed point |
| fresh_009 | 13/13 | 1437/1437 | 0 | 0 | 0 | 0 | fixed point |
| fresh_010 | 13/13 | 819/819 | 0 | 0 | 0 | 0 | fixed point |
| fresh_011 | 13/13 | 872/872 | 0 | 0 | 0 | 0 | fixed point |
| fresh_012 | 13/13 | 851/851 | 0 | 0 | 0 | 0 | fixed point |
| fresh_013 | 13/13 | 762/762 | 0 | 0 | 0 | 0 | fixed point |
| fresh_014 | 13/13 | 751/751 | 0 | 0 | 0 | 0 | fixed point |
| fresh_015 | 13/13 | 779/780 (1 Rust-only) | 0 | 0 | 0 | 0 | fixed point |
| fresh_016 | 13/13 | 751/751 | 0 | 0 | 0 | 0 | fixed point |
| fresh_017 | 13/13 | 779/779 | 0 | 0 | 0 | 0 | 49 differences |
| fresh_018 | 13/13 | 731/733 (2 Rust-only) | 0 | 0 | 0 | 0 | fixed point |
| fresh_019 | 13/13 | 1488/1488 | 0 | 0 | 0 | 0 | fixed point |
| fresh_020 | 13/13 | 775/775 | 0 | 0 | 0 | 0 | fixed point |
| fresh_021 | 13/13 | 783/783 | 0 | 0 | 0 | 0 | fixed point |
| fresh_022 | 13/13 | 766/766 | 0 | 0 | 0 | 0 | fixed point |
| fresh_023 | 13/13 | 1528/1528 | 0 | 0 | 0 | 0 | fixed point |
| fresh_024 | 13/13 | 788/788 | 0 | 0 | 0 | 0 | fixed point |
| fresh_025 | 13/13 | 869/869 | 0 | 0 | 0 | 0 | fixed point |
| fresh_026 | 13/13 | 839/839 | 0 | 0 | 0 | 0 | fixed point |
| fresh_027 | 13/13 | 796/796 | 0 | 0 | 0 | 0 | fixed point |
| fresh_028 | 13/13 | 794/794 | 0 | 0 | 0 | 0 | fixed point |
| fresh_029 | 13/13 | 817/817 | 0 | 0 | 0 | 0 | fixed point |
| fresh_030 | 13/13 | 755/755 | 0 | 0 | 0 | 0 | fixed point |
| fresh_031 | 13/13 | 774/774 | 0 | 0 | 0 | 0 | fixed point |
| fresh_032 | 13/13 | 760/760 | 0 | 0 | 0 | 0 | fixed point |
| fresh_033 | 13/13 | 770/770 | 0 | 0 | 0 | 0 | fixed point |
| fresh_034 | 13/13 | 832/832 | 0 | 0 | 0 | 0 | fixed point |
| fresh_035 | 13/13 | 745/745 | 0 | 0 | 0 | 0 | fixed point |
| fresh_036 | 13/13 | 845/845 | 0 | 0 | 0 | 0 | fixed point |
| fresh_037 | 13/13 | 739/739 | 0 | 0 | 0 | 0 | fixed point |
| fresh_038 | 13/13 | 1431/1431 | 0 | 0 | 0 | 0 | fixed point |
| fresh_039 | 13/13 | 778/778 | 0 | 0 | 0 | 0 | fixed point |
| fresh_040 | 13/13 | 1448/1448 | 0 | 0 | 0 | 0 | fixed point |
| fresh_041 | 13/13 | 788/788 | 0 | 0 | 0 | 0 | fixed point |
| fresh_042 | 13/13 | 876/876 | 0 | 0 | 0 | 0 | fixed point |
| fresh_043 | 13/13 | 765/765 | 0 | 0 | 0 | 0 | fixed point |
| fresh_044 | 13/13 | 833/833 | 0 | 0 | 0 | 0 | fixed point |
| fresh_045 | 13/13 | 793/793 | 0 | 0 | 0 | 0 | fixed point |
| fresh_046 | 13/13 | 785/785 | 0 | 0 | 0 | 0 | fixed point |
| fresh_047 | 13/13 | 777/777 | 0 | 0 | 0 | 0 | fixed point |
| fresh_048 | 13/13 | 824/824 | 0 | 0 | 0 | 0 | fixed point |
| fresh_049 | 13/13 | 786/786 | 0 | 0 | 0 | 0 | fixed point |
| fresh_050 | 13/13 | 768/768 | 0 | 0 | 0 | 0 | fixed point |
| fresh_051 | 13/13 | 822/822 | 0 | 0 | 0 | 0 | fixed point |
| fresh_052 | 13/13 | 769/769 | 0 | 0 | 0 | 0 | fixed point |
| fresh_053 | 13/13 | 823/823 | 0 | 0 | 0 | 0 | fixed point |
| fresh_054 | 13/13 | 798/798 | 0 | 0 | 0 | 0 | fixed point |
| fresh_055 | 13/13 | 805/805 | 0 | 0 | 0 | 0 | fixed point |
| fresh_056 | 13/13 | 756/756 | 0 | 0 | 0 | 0 | fixed point |
| fresh_057 | 13/13 | 831/831 | 0 | 0 | 0 | 0 | fixed point |
| fresh_058 | 13/13 | 799/799 | 0 | 0 | 0 | 0 | fixed point |
| fresh_059 | 13/13 | 766/766 | 0 | 0 | 0 | 0 | fixed point |
| fresh_060 | 13/13 | 740/740 | 0 | 0 | 0 | 0 | fixed point |
| fresh_061 | 13/13 | 801/801 | 0 | 0 | 0 | 0 | fixed point |
| fresh_062 | 13/13 | 788/788 | 0 | 0 | 0 | 0 | fixed point |
| fresh_063 | 13/13 | 782/782 | 0 | 0 | 0 | 0 | fixed point |
| fresh_064 | 13/13 | 752/752 | 0 | 0 | 0 | 0 | fixed point |
| fresh_066 | 13/13 | 1476/1476 | 0 | 0 | 0 | 0 | fixed point |
| fresh_067 | 13/13 | 792/792 | 0 | 0 | 0 | 0 | fixed point |
| fresh_068 | 13/13 | 1443/1443 | 0 | 0 | 0 | 0 | fixed point |
| fresh_071 | 13/13 | 744/744 | 0 | 0 | 0 | 0 | fixed point |
| fresh_072 | 13/13 | 768/768 | 0 | 0 | 0 | 0 | fixed point |
| fresh_073 | 13/13 | 772/772 | 0 | 0 | 0 | 0 | fixed point |
| fresh_075 | 13/13 | 790/790 | 0 | 0 | 0 | 0 | fixed point |
| fresh_076 | 13/13 | 795/795 | 0 | 0 | 0 | 0 | fixed point |
| fresh_077 | 13/13 | 833/833 | 0 | 0 | 0 | 0 | fixed point |
| fresh_078 | 13/13 | 812/812 | 0 | 0 | 0 | 0 | fixed point |
| fresh_079 | 13/13 | 970/970 | 0 | 0 | 0 | 0 | fixed point |
| fresh_080 | 13/13 | 787/787 | 0 | 0 | 0 | 0 | fixed point |
| fresh_081 | 13/13 | 736/736 | 0 | 0 | 0 | 0 | fixed point |
| fresh_082 | 13/13 | 949/949 | 0 | 0 | 0 | 0 | fixed point |
| fresh_083 | 13/13 | 771/771 | 0 | 0 | 0 | 0 | fixed point |
| fresh_084 | 13/13 | 741/741 | 0 | 0 | 0 | 0 | fixed point |
| fresh_085 | 13/13 | 804/804 | 0 | 0 | 0 | 0 | fixed point |
| fresh_074 | 13/13 | 758/758 | 0 | 0 | 0 | 0 | fixed point |
| fresh_090 | 13/13 | 820/820 | 0 | 0 | 0 | 0 | fixed point |
| fresh_086 | 13/13 | 763/763 | 0 | 0 | 0 | 0 | fixed point |
| fresh_087 | 13/13 | 821/821 | 0 | 0 | 0 | 0 | fixed point |
| fresh_088 | 13/13 | 792/792 | 0 | 0 | 0 | 0 | fixed point |
| fresh_089 | 13/13 | 1500/1500 | 0 | 0 | 0 | 0 | fixed point |
| fresh_091 | 13/13 | 846/846 | 0 | 0 | 0 | 0 | fixed point |
| fresh_092 | 13/13 | 956/956 | 0 | 0 | 0 | 0 | fixed point |
| fresh_093 | 13/13 | 764/764 | 0 | 0 | 0 | 0 | fixed point |
| fresh_094 | 13/13 | 825/825 | 0 | 0 | 0 | 0 | fixed point |
| fresh_096 | 13/13 | 759/759 | 0 | 0 | 0 | 0 | fixed point |
| fresh_097 | 13/13 | 763/763 | 0 | 0 | 0 | 0 | fixed point |
| fresh_098 | 13/13 | 796/796 | 0 | 0 | 0 | 0 | fixed point |
| fresh_100 | 13/13 | 807/807 | 0 | 0 | 0 | 0 | fixed point |
| fresh_101 | 13/13 | 763/763 | 0 | 0 | 0 | 0 | fixed point |
| fresh_102 | 13/13 | 860/860 | 0 | 0 | 0 | 0 | fixed point |
| fresh_103 | 13/13 | 773/773 | 0 | 0 | 0 | 0 | fixed point |
| fresh_104 | 13/13 | 832/832 | 0 | 0 | 0 | 0 | fixed point |
