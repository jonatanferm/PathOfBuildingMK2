# INT-1 parity triage (INT-1A diagnosis)

Unit INT-1A (calc doc §11.1 row INT-1, diagnosis phase), 2026-11-10. This is a diagnosis only: no
product code changed. The fixes are grouped into the fix units at the end. The result after
the fixes merged is in §8.

## 1. Baseline (confirmed)

`scripts/port/cargo.sh test --release -p pob-oracle --test CA -- ca_l6_corpus --nocapture`, run
from a worktree at rust-port `f9e53c4` (CS2 merged; full corpus, 57 builds). PoB runs were reused
from the main checkout's `target/tmp/ca-probe/` cache. That cache was written after the CA merge;
ORC-X2 changed only `state_dump.lua`'s dump format later, and L6 reads only `ca.json` /
`hits.json`.

| metric | measured | test floor |
|---|---|---|
| headline keys exact | **728 / 741** (56 of 57 builds have every headline key exact) | 728 |
| all main-output keys exact | **44,908 / 45,954** (97.72 %) | 44,908 |
| CALCS lists (Buff/Combat/CurseList) | **168 / 171** | — |
| used-tracking key sets | 504 / 504 (56 builds; Dual_Savior has no mainEnv, so it is not counted) | — |
| misc calculator | 1,226 / 1,226 (Dual_Savior's calculator raised, so it is not counted) | — |
| Rust errors | **4**: MAIN 1, CALCS 1, getMiscCalculator 1, configTab GetMiscCalculator 1; all upstream_Dual_Savior | 4 |

These numbers equal the floors CS2 set in `CA.rs`.

**Re-measured at rust-port `33851ba`** (fix round 1; FUP1 and U22 merged since `f9e53c4`). The
baseline is unchanged: 728/741 headline, 44,908/45,954 keys, 168/171 CALCS lists, 504/504
used-tracking, 1,226/1,226 misc, 4 Rust errors. FUP1's one `CA.rs` change (`flat_table` renders
`Key::F` float keys) adds no key on the corpus. Code refs below are to `33851ba`, where the
`flat_table` catch-all `_ => {}` moved from CA.rs:182 to CA.rs:183. The A-alone and A + C
counts (§3 Totals, §4 A, §5) were measured on `33851ba`; the B-alone and A + B + C counts are
from `f9e53c4`.

## 2. Method

1. **Per-key dump.** A local, uncommitted patch to `ca_l6_corpus` wrote every mismatching key per
   build (ours vs PoB) to a TSV. Grouping those keys by family and by build gives the buckets in §3.
2. **Rust errors.** I reran upstream_Dual_Savior with `CA_TRACE=1 RUST_BACKTRACE=1` to get the
   raising call chain.
3. **First diverging stage.** For each bucket I found the Lua statement where Rust's state first
   differs from PoB's, and confirmed it by experiment. Each experiment was a local, uncommitted
   change behind an env var: the candidate fix, plus `eprintln!` traces at the suspect Lua line
   (env identity, mode, `buildBreakdown`, pass `breakdown`). A bucket counts as diagnosed only
   when its candidate fix, applied alone, removes exactly that bucket's mismatches.
4. **Why L4 pairs were not the tool here.** I did not need the L4 segment pairs
   (`testkit::find_pair` / `segment_mismatches`), and neither real bug can show up in them:
   - Bucket A is a lifetime bug. An actor is freed when a GlobalCache entry is overwritten
     between two calcs. A single injected segment never contains that sequence.
   - Bucket C sits in the production closure of `callee("mirages", …)`. The PF-G L4 test's
     `replay_hook` answers that callee with PoB's recorded return value, so the closure never runs
     under L4 (`ailments_dispatch.rs:77-83`).

   Both buckets show only end to end (L5/L6). I added no `stages.toml` probes.
5. **Combined check.** All three candidate fixes applied together (§5) reach 100 % on every L6
   metric, and D13 holds on all 62 builds. So the buckets below are the complete explanation of
   the baseline gap.

## 3. Buckets, ordered by impact

| # | bucket | builds | keys affected | first diverging stage | owner unit / Rust file | Lua evidence | proposed fix | est. size |
|---|---|---|---|---|---|---|---|---|
| A | A copied skill's foreign `actor` is freed (a `Weak` back-pointer; Lua keeps the actor alive) | upstream_Dual_Savior | all 903 main keys fail at the baseline, including 13/13 headline. Only **899** are A's own: the raise masks 2 B and 2 C keys (see Totals); 3/3 CALCS lists; all **4 Rust errors**; 9 used-tracking tables and 22 misc values are not counted while it raises | `perform:end` of the MAIN env's perform (Calcs.lua:387). At CalcPerform.lua:3989, `cacheData` overwrites `GlobalCache.cachedData.MAIN[DoubleStrike uuid]`, which frees the env of the earlier `buildActiveSkill(DoubleStrike)` in Rust. The cached skill's actor then dies. It raises later at CalcActiveSkill.lua:181 → 236ff via Calcs.lua:403 → CalcPerform.lua:3798 → CalcMirages.lua:40 | **AS**: `calc_active_skill.rs` `copyActiveSkill` (584 passes `skill.actor.get()`, a `Weak`; raises at 713). The contract is U0's `active_skill.rs` `ActiveSkill.actor: Field<ActorWeak>` / calc doc §3.3 rule 4 | CalcActiveSkill.lua:181 `createActiveSkill(activeEffect, skill.supportList, skill.actor, …)` (a strong reference in Lua); CalcMirages.lua:134/137 return `GlobalCache…ActiveSkill`; CalcMirages.lua:155 `env.player.mainSkill = newSkill`; CalcPerform.lua:3989 | In `copyActiveSkill`, keep a strong reference to the source skill's actor on the new skill (§4 A) | S |
| B | `Req{Str,Dex,Int}Item.sourceGem` / `.sourceItem` missing on the Rust side (a **test-harness** representation gap, not a calc divergence) | at the baseline 48 builds, 143 keys (StrGem 34, IntItem 34, DexGem 27, DexItem 21, IntGem 14, StrItem 13). Once A is fixed: 49 builds, **145 keys**, because Dual_Savior's own 2 (`ReqDexItem.sourceGem`, `ReqStrItem.sourceItem`) stop being hidden by its raise. The other 8 builds have no `Req*Item` keys in PoB's output | none in the calc. Rust stores the right object; `CA.rs` `flat_table` skips every `Val::Obj` (CA.rs:183 `_ => {}`), while PoB's encoder writes `{"$gemInstance": nameSpec}` / `{"$item": name}` markers (`pob-oracle/src/oracle.lua:74`, `:85`) | **CA** (test): `crates/pob-oracle/tests/CA.rs` `flat_table` | CalcSetup.lua:1074 `sourceItem = item`, 1847 `sourceGem = gemInstance`; CalcPerform.lua:2128 `out.source = reqSource`, 2156 `output["Req"..attr.."Item"] = out.source` | In `flat_table`, render `GemInstance` / `Item` objects as the oracle's marker strings. Verified: with that rendering, all 143 baseline keys are equal (and all 145 once A is fixed, §5) | XS |
| C | `calcs.mirages`' return value is discarded, so the MAIN env reruns `offence` on the Reflection mirage skill. It shows only once A is fixed | upstream_Dual_Savior | 2: `MainHand.BleedDotMulti`, `OffHand.BleedDotMulti`, Rust 1.002750 vs PoB 1.000000 | `perform:3497`→`perform:end` (PF-G), CalcPerform.lua:3798-3799. `mirages` returns `true` for Reflection, so PoB skips `offence`; Rust always runs it (traced: `offence` runs in the MAIN env right after `reflection post`). Because `env.player.output` is the mirage env's output table (CalcMirages.lua:157), the rerun writes into the table that becomes the main output. With breakdown on in MAIN, the breakdown-only reweight at CalcOffence.lua:4602-4609 turns `1` into `1·(1−0.0055) + 1.5·0.0055 = 1.00275` (CritBleedDotMulti 1.5, crit chance 0.55 %) | **PF-G**: `calc_perform/ailments_dispatch.rs:1187-1190` | CalcMirages.lua:57 `return not config.calcMainSkillOffence` (Reflection and Tawhoa's Chosen set no `calcMainSkillOffence`); CalcPerform.lua:3798 `if not calcs.mirages(env) then calcs.offence(…) end` | The documented, never-applied CCR-0090 §1 follow-up: `callee("mirages", …, \|\| Val::Bool(calcs::mirages(&env)))` | XS |

**Totals.** At the baseline, Dual_Savior's raise makes all of its 903 keys (13 headline), 3
CALCS lists and 4 errors fail, and B makes 143 keys on the other builds fail. 903 + 143 = 1,046 =
45,954 − 44,908. But the raise **masks** 4 keys that belong to other buckets: Dual_Savior's 2 B
keys and its 2 C keys. So A by itself fixes **899** keys, not 903. Per-fix deltas on the full
corpus (§5; A and A + C on `33851ba`, B alone and A + B + C on `f9e53c4`, same baseline):

| applied | keys exact | Dual_Savior | gained by the last fix |
|---|---|---|---|
| baseline | 44,908 | 0/903 (raises) | — |
| A | 45,807 | 899/903 (2 B + 2 C left) | A: +899 |
| A + C | 45,809 | 901/903 (2 B left) | C: +2 |
| A + B + C | 45,954 | 903/903 | B: +145 |
| B alone | 45,051 | 0/903 (raises) | B: +143 (Dual_Savior's 2 still masked) |

899 + 2 + 145 = 1,046. Dual_Savior reaches 903/903 only with all three of A, B and C.

**Latent, outside the corpus.** C also affects any perform whose main skill is **Tawhoa's Chosen**
(CalcMirages.lua:165-283 builds a config without `calcMainSkillOffence`). No corpus build has it
as main skill. General's Cry builds no config, so `mirages` returns nil there and `offence` runs
in both implementations; upstream_Generals_Perforate_Zerker is exact.

## 4. Bucket details

### A. The foreign actor of a copied skill dies (upstream_Dual_Savior)

The main socket group is Reflection (The Saviour, Weapon 1). The sequence below is derived from
the Lua and confirmed by Rust traces of env identities:

1. `buildOutput(MAIN)` → `perform(env₀)` → CalcPerform.lua:3798 `calcs.mirages` → Reflection
   `compareFunc`. DoubleStrike is not cached, so `buildActiveSkill` creates env₁, performs it, and
   caches `MAIN[DS] = {Env = env₁, ActiveSkill = DS₁}` (Common.lua:924-941 `cacheData`). CalcMirages.lua:40
   then copies DS₁, producing `newSkill₁` with `actor = DS₁.actor = env₁.player`.
2. `postCalcFunc` (CalcMirages.lua:155) sets `env₀.player.mainSkill = newSkill₁`. At
   CalcPerform.lua:3989, `cacheData(cacheSkillUUID(newSkill₁), env₀)` files the result under the
   **DoubleStrike** uuid (FOLLOWUPS "CA" 4 measured this in PoB) and replaces `MAIN[DS]`.
   - In Lua, env₁.player stays alive through `newSkill₁.actor`.
   - In Rust, nothing strong holds env₁ any more (`CachedData.Env` was the only owner), and
     `ActiveSkill.actor` is a `Weak`. So `newSkill₁.actor()` is `None` from here on. This is the
     first state divergence.
3. The cost-warning loop (Calcs.lua:400-403) calls `buildActiveSkill(Reflection)`. Its
   `compareFunc` returns `MAIN[DS].ActiveSkill = newSkill₁` (CalcMirages.lua:134/137), and
   `copyActiveSkill` passes the dead `Weak` on (CalcActiveSkill.lua:181). `buildActiveSkillModList`
   then raises `attempt to index field 'actor' (a nil value)` (calc_active_skill.rs:713).
   - Backtrace: `buildOutput` → `buildOutput_costWarnings` → `buildActiveSkill` → `perform` →
     `ailments_dispatch::seg` → `calc_mirages::mirages` → `copyActiveSkill` →
     `buildActiveSkillModList`.
4. The rest of the build's errors follow from that one raise:
   - CALCS raises `attempt to index field 'mainEnv'` (Calcs.lua:635, `calcs.rs` `mainEnv_multiplierUsed`)
     because MAIN never set `calcsTab.mainEnv`.
   - `getMiscCalculator` and configTab's `GetMiscCalculator` raise the same 'actor' error, through
     the same mirage path inside `calcFullDPS` in CALCULATOR mode.

This corrects FOLLOWUPS "CA" 6. That item says the actor of the **mirage** env is lost. In fact
the actor that dies belongs to the **source** skill's env (the earlier `buildActiveSkill` env,
freed by the cache overwrite). The fix therefore belongs in `copyActiveSkill` (AS). Keeping the
mirage env alive (MI) would not help.

**Experiment.** In `copyActiveSkill`, after `createActiveSkill`, I stored `skill.actor()` (or the
source's kept actor) strongly in `newSkill.extra`, but only when that actor is not `env.player`
(the guard proposed below). With A alone, Dual_Savior reached 13/13 headline, 0 errors,
**899/903** keys, 3/3 CALCS lists, 9/9 used-tracking tables and 22/22 misc values. Four keys are
left:
- `MainHand.BleedDotMulti`, `OffHand.BleedDotMulti`: bucket C;
- `ReqDexItem.sourceGem` (PoB `{"$gemInstance":"Double Strike"}`) and `ReqStrItem.sourceItem`
  (PoB `{"$item":"The Saviour, Legion Sword"}`): bucket B, hidden behind the raise until now.

With A + C the build is at 901/903, and only the 2 B keys are left. It reaches 903/903 only with
A, B and C together. (The first version of this report said A alone gives 901/903 with only C
left. That was wrong: it was the A + C figure.)

**Proposed fix (AS) and contract note.**
- Lua's `skill.actor` reference is strong. Calc doc D2 names exactly this case, but §3.3 rule 4
  makes `ActiveSkill.actor` a `Weak`.
- Proposed: `copyActiveSkill` keeps an owning reference to the foreign actor on the copy, but only
  when that actor is not `env.player`. A copy of a skill of the same env must not get one:
  `postCalcFunc` would store the copy in `env.player.mainSkill`, and the strong reference would
  close an `Rc` cycle.
- The fix unit should file a CCR (number from `scripts/port/next_ccr.sh`) amending §3.3 rule 4.
  There are two options:
  - an owning `ActiveSkill.actorKeep: Field<Option<ActorRef>>`, which touches the frozen
    `active_skill.rs` (U0) and needs coordination with RETYPE;
  - until then, a `// CCR-NNNN` workaround through `newSkill.extra`, as in the experiment.
- Regression gate: `ca_l6_corpus` and `ca_d13_fixed_point` on upstream_Dual_Savior (0 errors), plus
  a leak check: hold a `Weak` to the source env's player, then drop the build and call
  `wipeGlobalCache()`; the `Weak` must be dead. A cycle would keep it alive.
- The `env.player` guard is argued here, not proved. The experiment used it and got the corpus
  results above, but the corpus cannot show a leak. So INT-1B must add that leak test as a
  committed test, and it should cover both cases: a copy from a foreign env (the `Weak` must die
  after the drop and wipe) and a copy of a same-env skill (no strong reference is stored at all).

### B. `Req*Item.source*` (harness)

- Rust writes the right objects:
  - `init_env_items.rs:1008` (`sourceItem`, CalcSetup.lua:1074);
  - `init_env_skills.rs:1227` (`sourceGem`, CalcSetup.lua:1847);
  - `pools_charges.rs` (CalcPerform.lua:2118-2156).
- `CA.rs`'s `flat_table` drops `Val::Obj` values, so the 143 PoB marker strings have no Rust
  counterpart.
- Experiment: rendering `Val::Obj` as `{"$gemInstance": GemInstance.nameSpec}` /
  `{"$item": Item.name}` (the oracle's `refFor`, `src/oracle.lua:70-92`) made all 143 equal.
  Keys went from 44,908 to 45,051 with no other change.
- Once A is fixed, B is 145 keys on 49 builds: Dual_Savior has 2 B keys of its own
  (`ReqDexItem.sourceGem` = Double Strike, `ReqStrItem.sourceItem` = The Saviour), which its raise
  hid at the baseline. A + C without B measures 45,809 = 45,954 − 145.
- Not a calc bug. It needs no CCR.

### C. `calcs.mirages` return discarded (PF-G)

- `calc_mirages.rs:418` already returns `bool` (CCR-0090 §1, applied by MI).
- PF-G's call site is unchanged: it still runs `calcs::mirages(&env); Val::Nil` (the old CCR-0077
  §2 workaround). CCR-0090 §1 lists the one-line follow-up as "Needed follow-up (PF-G)", and it
  was never done.
- Traces (Dual_Savior, MAIN env `e50`):
  1. `reflection post: env=e50 MAIN newEnv=440 CALCULATOR`.
  2. Then `bleed L4602: env=e50 mode=MAIN buildBreakdown=true pass.breakdown=true
     BleedDotMulti=1 Crit=1.5`, twice (MainHand and OffHand).
  3. Then `MAIN buildOutput returned env=e50 BleedDotMulti=1.00275`.

  PoB's mirage env runs in CALCULATOR mode (`buildBreakdown` false, CalcSetup.lua:500), and PoB
  never reruns offence in env₀. So its value stays 1.
- Experiment: with the call site returning `Val::Bool(calcs::mirages(&env))` on top of A,
  Dual_Savior's 2 `BleedDotMulti` keys match (899 → 901/903; the 2 left are B). C alone changes
  nothing measurable, because Dual_Savior still raises.
- The other offence outputs that the rerun rewrites happen to get the same values, but the rerun
  also fills the MAIN env's offence **breakdown** for the mirage skill. INT-3 (breakdown parity)
  would therefore see this bug too.

## 5. All three fixes together (local experiment)

Intermediate states (env-gated, uncommitted, rust-port `33851ba`, full corpus):

```
A:      0 Rust runs raised; headline 741/741; all keys 45807/45954; CALCS lists 171/171;
        used-tracking 513/513; misc 1248/1248   (left: 145 B keys on 49 builds + 2 C keys)
A + C:  0 Rust runs raised; headline 741/741; all keys 45809/45954; CALCS lists 171/171;
        used-tracking 513/513; misc 1248/1248   (left: the 145 B keys only)
```

With A + B + C applied locally (env-gated, uncommitted; measured on `f9e53c4`):

```
CA L6: 57 builds (0 PoB runs failed), 0 Rust runs raised; headline 741/741 exact (100.0%),
57 builds with every headline key exact; all keys 45954/45954 exact (100.00%); CALCS lists
171/171; used-tracking key sets 513/513; misc calculator 1248/1248
CA D13: 62 builds, 62 ran, 62 fixed points, 0 differ, 0 Lua errors; 1 warm-cache runs checked
against PoB's
```

The bounds can then become the hard gate FOLLOWUPS "CA" 1 asks for: `L6_MIN_HEADLINE_EXACT = 741`,
`L6_MIN_KEYS_EXACT = 45_954`, `L6_MAX_RUST_ERRORS = 0`, every `L6_MAX_STAGE_ERRORS` entry 0, and
`D13_MAX_ERRORS = 0`. `L5_MAX_LIMITED[0]` (Dual_Savior's cost block in `ca_l5_post`) is a harness
limit that does not depend on A, so it stays.

## 6. PoB nondeterminism and host artefacts (no mismatch; listed separately)

- **`pairs` order / run-to-run values.** `.parity/nondet/*.json` records no `outputs` or
  `outputOrders` entry for any of the 57 builds, so no L6 mismatch comes from PoB nondeterminism.
- **known_diffs.toml.** No entry applies at L6. All entries are parse-level or state/breakdown
  paths of L4 stages: OF-A-1, MP*, DF-E-1, PF-C-1, DF-G-1.
- **±0 (CCR-0001/0002).** 58 L6 keys pass **only** through the sign-of-zero tolerance
  (`num_eq`): `VaalArcticArmourMitigation` on all 57 builds, plus `Minion.VaalArcticArmourMitigation`
  on allflame_exp_039.
  - In every case Rust has `-0` and PoB `+0`. CalcDefence.lua:2576 computes
    `m_min(-modDB:Sum("MORE", nil, "VaalArcticArmourMitigation") / 100, 1)` with Sum = 0. That gives
    `-0` in x64 doubles, which is the profile the port follows; the aarch64 dual-number oracle
    gives integer `0`.
  - Host artefact; no action.
  - At the baseline this is 57 keys, because Dual_Savior raised. The non-Savior keys do not depend
    on fixes A-C.
- **Warm-cache D13 (FOLLOWUPS "CA" 5).** allflame_exp_050 differs in PoB itself on a warm
  GlobalCache. That is PoB behaviour, and Rust reproduces it (`D13_POB_WARM_CACHE`). The build
  is exact at L6.

## 7. Fix units

The fix units have disjoint files and can run in parallel. The only ordering is that INT-1D's
final floor values need INT-1B and INT-1C merged first; its harness change does not. Each unit's
key target below is the expected delta from the table under §3 "Totals"; the units' gates must
not expect more than their own bucket gives.

| unit | buckets | owned files (only these) | acceptance | size |
|---|---|---|---|---|
| **INT-1B** (owner AS) | A | `crates/pob-calc/src/modules/calc_active_skill.rs` (`copyActiveSkill`); a new CCR `docs/port/ccr/NNNN-int-1b-foreign-actor-keepalive.md` (number from `next_ccr.sh`) amending calc doc §3.3 rule 4 / `ActiveSkill.actor`. Use a `// CCR-NNNN` workaround via `newSkill.extra`, so frozen `active_skill.rs` stays untouched while RETYPE runs | `CA_BUILDS=upstream_Dual_Savior` `ca_l6_corpus`: 0 Rust errors, 13/13 headline, ≥ 899/903 keys alone (left: 2 C + 2 B keys), ≥ 901/903 with INT-1C, and 903/903 only once INT-1D's `flat_table` change is in too; full corpus ≥ 45,807 keys alone; `ca_d13_fixed_point` 0 errors; the committed leak test of §4 A (foreign-env copy releases its actor after drop + `wipeGlobalCache()`; a copy of a same-env skill keeps no strong reference, so no `Rc` cycle); AS.rs / MI.rs suites unchanged | S |
| **INT-1C** (owner PF-G) | C | `crates/pob-calc/src/modules/calc_perform/ailments_dispatch.rs` (L3798 call site, the CCR-0090 §1 follow-up; drop the CCR-0077 §2 workaround comment) | PF-G.rs suite unchanged (its replay already returns PoB's value); with INT-1B, Dual_Savior `MainHand/OffHand.BleedDotMulti` = 1 (Dual_Savior 899 → 901/903, full corpus 45,807 → 45,809). Alone it changes no L6 count | XS |
| **INT-1D** (owner CA) | B + gates | `crates/pob-oracle/tests/CA.rs` (`flat_table` renders `GemInstance`/`Item` objects as oracle markers; then the floors/bounds of §5); `docs/port/FOLLOWUPS.md` "CA" items 1 and 6 (correct the item 6 cause, close both once green) | full-corpus `ca_l6_corpus` ≥ 45,051 keys with the harness change alone (+143; Dual_Savior's 2 B keys stay masked until INT-1B). After INT-1B and INT-1C merge: 741/741, 45,954/45,954 (+145 over INT-1B + INT-1C's 45,809), 171/171, 0 errors as hard gates, `D13_MAX_ERRORS = 0`. Re-measure every floor on the rust-port head INT-1D starts from (this report's base moved from `f9e53c4` to `33851ba` during review, with the baseline unchanged) | XS |

Three units are enough for this baseline; nothing else diverges at L6 on the corpus. The calc doc
row's next gate is "L6 on all57 + 100 fresh poe.ninja builds". That needs a fresh corpus and a new
triage round once these three land.

## 8. Final result (INT-1-B, 2026-11-11)

The fix units ran under the plan's ids, not the names of §7: **INT-1-A** = §7's INT-1B (bucket A,
AS, CCR-0113), **INT-1-C** = §7's INT-1C (bucket C, PF-G), **INT-1-B** = §7's INT-1D (bucket B
and the gates, CA). INT-1-B ran on rust-port `4870179` (INT-1-A, INT-1-C and RETYPE merged).

**Bucket B.** `CA.rs` `flat_table` renders an `Item` as `{"$item": name}` and a `GemInstance`
as `{"$gemInstance": nameSpec}` (`obj_marker`, the oracle's `refFor`, `src/oracle.lua:70-92`),
as the one-member marker string `oracle::flatten` produces. Other calc objects stay skipped.

**Measured** (`POB_FULL_SUITE=1 scripts/port/cargo.sh test --release -p pob-oracle --test CA`,
PoB runs from the main checkout's `target/tmp/ca-probe/` cache, written after the CA merge):

```
CA L6: 57 builds (0 PoB runs failed), 0 Rust runs raised; headline 741/741 exact (100.0%),
57 builds with every headline key exact; all keys 45954/45954 exact (100.00%); CALCS lists
171/171; used-tracking key sets 513/513; misc calculator 1248/1248
CA D13: 62 builds, 62 ran, 62 fixed points, 0 differ, 0 Lua errors; 1 warm-cache runs checked
against PoB's
CA L5: cost warnings 56 clean (1 harness-limited), used-tracking 55 clean (2 harness-limited),
CALCS lists 57 clean; 0 failed
```

Control: with the marker rendering switched off (a local env gate, not committed) on the same
head, L6 gives 45,809/45,954 keys, 49 builds not exact, Dual_Savior 901/903, everything else
unchanged. So B is the predicted +145, and A + C were already in at 45,809.

**Gates now in `CA.rs`.**
- `ca_l6_corpus`: no Rust error, and headline, all keys (Rust-only keys count as not exact),
  CALCS lists, used-tracking key sets and misc calculator values all exact, on any corpus
  subset. On the full corpus the compared totals are pinned too (`L6_BUILDS = 57`,
  `L6_HEADLINE_KEYS = 741`, `L6_KEYS = 45_954`, `L6_CALCS_LISTS = 171`,
  `L6_USED_TABLES = 513`, `L6_MISC = 1_248`), so a value missing on both sides fails. The
  floors `L6_MIN_*`, `L6_MAX_RUST_ERRORS` and `L6_MAX_STAGE_ERRORS` are removed.
- `ca_d13_fixed_point`: no build may raise (`D13_MAX_ERRORS` removed). It now also checks a
  CALCS fixed point (FOLLOWUPS "CA" 1): `calcsTab.mainEnv` set to the MAIN env, then two CALCS
  `buildOutput`s with equal outputs and equal `SaveDB` between them. The first CALCS run can
  change the model relative to MAIN (CalcSetup.lua:1883 clamps `mainActiveSkillCalcs`), so the
  state is compared between the two CALCS runs. `D13_POB_WARM_CACHE` (allflame_exp_050) gets a
  wipe before each CALCS run, as for MAIN.
- `L5_MAX_LIMITED = [1, 2, 0]` in `ca_l5_post` is unchanged (harness limits, §5).

INT-1's all57 gate is met. The other half of INT-1's gate (L6 on 100 fresh poe.ninja builds) is
a separate follow-up: it needs a fresh corpus and a new triage round.
