# INT-1 parity triage (INT-1A diagnosis)

Unit INT-1A (calc doc §11.1 row INT-1, diagnosis phase), 2026-11-10. This is a diagnosis only: no
product code changed. The fixes are grouped into the fix units at the end.

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
| A | A copied skill's foreign `actor` is freed (a `Weak` back-pointer; Lua keeps the actor alive) | upstream_Dual_Savior | all 903 main keys, including 13/13 headline; 3/3 CALCS lists; all **4 Rust errors**; 9 used-tracking tables and 22 misc values are not counted while it raises | `perform:end` of the MAIN env's perform (Calcs.lua:387). At CalcPerform.lua:3989, `cacheData` overwrites `GlobalCache.cachedData.MAIN[DoubleStrike uuid]`, which frees the env of the earlier `buildActiveSkill(DoubleStrike)` in Rust. The cached skill's actor then dies. It raises later at CalcActiveSkill.lua:181 → 236ff via Calcs.lua:403 → CalcPerform.lua:3798 → CalcMirages.lua:40 | **AS**: `calc_active_skill.rs` `copyActiveSkill` (584 passes `skill.actor.get()`, a `Weak`; raises at 713). The contract is U0's `active_skill.rs` `ActiveSkill.actor: Field<ActorWeak>` / calc doc §3.3 rule 4 | CalcActiveSkill.lua:181 `createActiveSkill(activeEffect, skill.supportList, skill.actor, …)` (a strong reference in Lua); CalcMirages.lua:134/137 return `GlobalCache…ActiveSkill`; CalcMirages.lua:155 `env.player.mainSkill = newSkill`; CalcPerform.lua:3989 | In `copyActiveSkill`, keep a strong reference to the source skill's actor on the new skill (§4 A) | S |
| B | `Req{Str,Dex,Int}Item.sourceGem` / `.sourceItem` missing on the Rust side (a **test-harness** representation gap, not a calc divergence) | 56 (every build except Dual_Savior) | 143 keys (StrGem 34, IntItem 34, DexGem 27, DexItem 21, IntGem 14, StrItem 13) | none in the calc. Rust stores the right object; `CA.rs` `flat_table` skips every `Val::Obj` (CA.rs:181-182 `_ => {}`), while PoB's encoder writes `{"$gemInstance": nameSpec}` / `{"$item": name}` markers (`pob-oracle/src/oracle.lua:74`, `:85`) | **CA** (test): `crates/pob-oracle/tests/CA.rs` `flat_table` | CalcSetup.lua:1074 `sourceItem = item`, 1847 `sourceGem = gemInstance`; CalcPerform.lua:2128 `out.source = reqSource`, 2156 `output["Req"..attr.."Item"] = out.source` | In `flat_table`, render `GemInstance` / `Item` objects as the oracle's marker strings. Verified: with that rendering, all 143 keys are equal | XS |
| C | `calcs.mirages`' return value is discarded, so the MAIN env reruns `offence` on the Reflection mirage skill. It shows only once A is fixed | upstream_Dual_Savior | 2: `MainHand.BleedDotMulti`, `OffHand.BleedDotMulti`, Rust 1.002750 vs PoB 1.000000 | `perform:3497`→`perform:end` (PF-G), CalcPerform.lua:3798-3799. `mirages` returns `true` for Reflection, so PoB skips `offence`; Rust always runs it (traced: `offence` runs in the MAIN env right after `reflection post`). Because `env.player.output` is the mirage env's output table (CalcMirages.lua:157), the rerun writes into the table that becomes the main output. With breakdown on in MAIN, the breakdown-only reweight at CalcOffence.lua:4602-4609 turns `1` into `1·(1−0.0055) + 1.5·0.0055 = 1.00275` (CritBleedDotMulti 1.5, crit chance 0.55 %) | **PF-G**: `calc_perform/ailments_dispatch.rs:1187-1190` | CalcMirages.lua:57 `return not config.calcMainSkillOffence` (Reflection and Tawhoa's Chosen set no `calcMainSkillOffence`); CalcPerform.lua:3798 `if not calcs.mirages(env) then calcs.offence(…) end` | The documented, never-applied CCR-0090 §1 follow-up: `callee("mirages", …, \|\| Val::Bool(calcs::mirages(&env)))` | XS |

**Totals.** At the baseline, A accounts for 903 keys (13 headline), 3 CALCS lists and 4 errors,
and B for 143 keys. 903 + 143 = 1,046 = 45,954 − 44,908. C (2 keys) is hidden behind A's raise
and appears once A is fixed (§5).

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
source's kept actor) strongly in `newSkill.extra`. Dual_Savior then reached 13/13 headline, 0
errors, 901/903 keys, 3/3 CALCS lists, 9/9 used-tracking tables and 22/22 misc values. The 2 keys
left are bucket C.

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
- Experiment: with the call site returning `Val::Bool(calcs::mirages(&env))`, Dual_Savior's last
  2 keys match (§5).
- The other offence outputs that the rerun rewrites happen to get the same values, but the rerun
  also fills the MAIN env's offence **breakdown** for the mirage skill. INT-3 (breakdown parity)
  would therefore see this bug too.

## 5. All three fixes together (local experiment)

With A + B + C applied locally (env-gated, uncommitted):

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
final floor values need INT-1B and INT-1C merged first; its harness change does not.

| unit | buckets | owned files (only these) | acceptance | size |
|---|---|---|---|---|
| **INT-1B** (owner AS) | A | `crates/pob-calc/src/modules/calc_active_skill.rs` (`copyActiveSkill`); a new CCR `docs/port/ccr/NNNN-int-1b-foreign-actor-keepalive.md` (number from `next_ccr.sh`) amending calc doc §3.3 rule 4 / `ActiveSkill.actor`. Use a `// CCR-NNNN` workaround via `newSkill.extra`, so frozen `active_skill.rs` stays untouched while RETYPE runs | `CA_BUILDS=upstream_Dual_Savior` `ca_l6_corpus`: 0 Rust errors, 13/13 headline, ≥ 901/903 keys before INT-1C and 903/903 with it; `ca_d13_fixed_point` 0 errors; no `Rc` cycle (copy of a same-env skill keeps no strong reference); AS.rs / MI.rs suites unchanged | S |
| **INT-1C** (owner PF-G) | C | `crates/pob-calc/src/modules/calc_perform/ailments_dispatch.rs` (L3798 call site, the CCR-0090 §1 follow-up; drop the CCR-0077 §2 workaround comment) | PF-G.rs suite unchanged (its replay already returns PoB's value); with INT-1B, Dual_Savior `MainHand/OffHand.BleedDotMulti` = 1 | XS |
| **INT-1D** (owner CA) | B + gates | `crates/pob-oracle/tests/CA.rs` (`flat_table` renders `GemInstance`/`Item` objects as oracle markers; then the floors/bounds of §5); `docs/port/FOLLOWUPS.md` "CA" items 1 and 6 (correct the item 6 cause, close both once green) | full-corpus `ca_l6_corpus` ≥ 45,051 keys with the harness change alone. After INT-1B and INT-1C merge: 741/741, 45,954/45,954, 171/171, 0 errors as hard gates, `D13_MAX_ERRORS = 0` | XS |

Three units are enough for this baseline; nothing else diverges at L6 on the corpus. The calc doc
row's next gate is "L6 on all57 + 100 fresh poe.ninja builds". That needs a fresh corpus and a new
triage round once these three land.
