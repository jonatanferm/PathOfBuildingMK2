# INT-3 breakdown parity triage

Unit INT-3 (calc doc §11.1 row INT-3: "CALCS-mode breakdown tables structurally equal on all57;
Tabulate after calc equal"), 2026-11-12, on rust-port `fa733f3`. The unit adds the harness
(`crates/pob-oracle/tests/INT-3.rs`, `crates/pob-oracle/corpus/INT-3/int3.lua`), measures, and
triages. **Result: no difference is a port bug.** Every difference the first measurement found is
one of: a harness representation gap (fixed in the harness), a host artefact already on record
(CCR-0001), or PoB's own per-process nondeterminism (calc doc §3.4 / D8). PoB-vs-PoB runs confirm
the nondeterminism (§4). So §3 has no product-code bucket and no follow-up unit is needed.
No product file changed.

## 1. What is compared

Both tests reproduce PoB's load end to end, as `ca_l6_corpus` does: the Rust build model from the
XML, then `CalcsTab:BuildOutput` once per BuildOutput PoB ran (PoB's `ca:399` hit count / 2;
Build.lua 693-697 and 1258-1273). Then they compare what the Calcs tab reads from
`calcsTab.calcsEnv`. The breakdown control only ever reads `calcsEnv`: `self.envName` is never set,
CalcBreakdownControl.lua:50.

* **`int3_breakdown_corpus`** compares `calcsEnv.player.breakdown` and `calcsEnv.minion.breakdown`.
  - PoB side: `int3.lua` writes `calcsEnv` with state_dump.lua's `encodeDoc`.
  - Rust side: `pob_calc::probe::export_root`, with the two breakdowns as locals.
  - Both trees are normalised the same way: `$ref` resolved, items reduced to `{"$item": name}`,
    stores to `{"$store": class}`, actors and active skills to markers.
  - Each **entry** is compared as a whole tree, with numbers within §9.2 tolerance. An entry is a
    top-level key, or a key under the `MainHand` / `OffHand` namespaces (CalcBreakdownControl.lua:150-155).
    So text lines, `rowList`/`colList`, `slots`, `reservations`, `damageTypes`, `modList` rows
    (mod + value), `label`, `footer` and `radius` are all compared.
  - An entry is *equal* if it matches after sorting the lists the control sorts before display:
    `rowList` of an entry with a `source` (181-190), `slots` (263-265) and `modList` rows (337-345).
    It is *exact* if it also matches in order.
* **`int3_tabulate_corpus`** runs every distinct Modules/CalcSections.lua query that AddModSection
  makes (`modStore:Tabulate(modType, cfg, modName…)`, CalcBreakdownControl.lua:303-318), on the
  player and on the minion when there is one.
  - The queries are collected by `int3.lua` with a depth-first walk over the entries with a
    `modName` and no `breakdown`.
  - Each query also gets the per-source `Combine` totals of the source tooltips (348-381).
  - Rows (value + mod) are compared in order, else as a multiset ("equal up to row order"), and
    so are the totals. A query that raises in PoB must also raise in Rust.

Run: `POB_FULL_SUITE=1 scripts/port/cargo.sh test --release -p pob-oracle --test INT-3 --
--test-threads 1` with `POB_PARITY_DIR=<main>/.parity`. PoB runs are cached in
`target/tmp/int3-probe/<stem>/`. `INT3_DIFFS=<file>` writes every difference, every accepted
difference and every order-only entry as TSV. `INT3_STRICT=1` turns off the PoB-nondeterminism
acceptances (§3 D-F).

## 2. Results (measured)

| measurement | first run (harness as first written) | final |
|---|---|---|
| builds / PoB runs failed / Rust errors | 57 / 0 / 0 | 57 / 0 / 0 |
| breakdown entries equal | 10,596 / 10,606 | **10,606 / 10,606** |
| … exact in order | 10,523 | 10,527 |
| … missing in Rust / only in Rust | 0 / 0 | 0 / 0 |
| Tabulate queries equal | 38,690 / 38,940 | **38,940 / 38,940** |
| … equal up to row order | 134 | 129 |
| … equal up to PoB's nondeterministic sources (§3 E, F) | — | 6 |
| PoB Tabulate rows compared | 42,588 | 42,588 |
| test binary | — | `ok. 2 passed` (29.3 s on cached PoB runs, peak RSS 1.48 GB) |

The accepted classes in the final run were: 65 `%d` host casts (§3 A), 498 row closures (§3 H2),
0 nondeterministic gem names (the PoB run in the cache happened to pick Rust's variant; §3 D),
and 6 nondeterministic sources (§3 E, F). Pins in `INT-3.rs`, used as full-corpus floors:
`BD_FULL_ENTRIES` = 10,606, `TAB_FULL_QUERIES` = 38,940, `TAB_FULL_ROWS` = 42,588.

With `INT3_STRICT=1` on the same cached PoB runs, the result is 10,606 / 10,606 entries and
38,934 / 38,940 queries. The 6 queries that fail are exactly §3 E and F. The cached Cospris run
happens to pick Rust's gem variant, so D does not show.

The first-run counts come from the first full run, before the harness fixes (H1, H2) and before
the documented acceptances. They differ from the final run in two ways. First, H2's 498 closures
were already accepted in that run. Second, the PoB caches of 8 builds were later refreshed by the
PoB-vs-PoB runs (§4), which moved some order-only counts.

## 3. Buckets

| # | bucket | what it is in the first run | Lua evidence | disposition |
|---|---|---|---|---|
| H1 | harness: `__oracle_json` omits nil values, so a `Combine` total that is nil (`FLAG` with no mod for that source; ModStore.lua `Combine` returns `Flag`'s `nil`) has no `total` key in PoB's JSON. Rust wrote `"total": null`. Also, an empty Lua list is encoded as `{}` | 244 of the 250 differing queries (totals only) | oracle.lua `encode` (nil → field dropped; empty table → `{}`) | fixed in the harness: no `total` key when the total is nil; `{}` read as an empty list |
| H2 | harness: a Lua closure in a breakdown row (`sourceNameTooltip`, CalcPerform.lua:2139-2141) is a calc object (`Val::Obj(SourceNameTooltip)`, pools_charges.rs) in Rust. The state export writes it as `{"$userdata": true}`; PoB's dump writes `{"$fn": "Modules/CalcPerform.lua:2139-2141"}` | 498 rows in 3 entries per build (ReqStr/Dex/Int) | CalcPerform.lua:2139 | accepted as "closure present" (`Accepted::closure`). The closure's behaviour is UI (U40), not compared here |
| H3 | harness: a LIST value's inner mod (`SkillBuffs` `MinionModifier` rows) carries `"tags": []` in PoB's dump and no `tags` in the Rust export | 0 on all57; 4 entries on the fresh corpus (§5) | Calcs.lua:765-770; state_dump.lua `encodeMod` | fixed in the harness (`norm`) |
| A | host artefact: `s_format("%d", x)` of a NaN / ±inf `x`. x64 PoB (cvttsd2si, the profile the port follows) prints `-9223372036854775808`; the ARM64 oracle saturates the cast (`0` / `±9223372036854775807`) | 65 strings, e.g. `FrostShieldLife[5]` in every build, plus the `MaximumHitTaken` / `DotEHP` `%d` lines | CalcDefence.lua:2498 (`lifeProtected = 0/(0/100)*1`), 3331-3343, 3376-3378 | accepted on aarch64 only (`Accepted::int_cast`). Already recorded: known_diffs.toml `DF-E-1`, `DF-G-1`; CCR-0001, AMENDMENTS A6 |
| B | LUA-ORDER: `<type>MaximumHitTaken.rowList` is filled by `pairs(takenDamages)`, which has string keys | 8 entries in 6 builds (5 Physical, 1 each of Cold, Fire, Lightning) | CalcDefence.lua:3325 | compared as a multiset (`ui_sorted`) |
| B2 | LUA-ORDER (found at the INT-3 merge, fresh PoB runs): `TotalBuildDegen.rowList` is filled by `pairs(actor.damageOverTimeShiftTable[damageType])`, string keys; allflame_exp_002's Physical / Fire rows swapped in 1 of 3 fresh PoB runs | 1 entry in 1 build | CalcDefence.lua:3389 | compared as a multiset (`ui_sorted`) |
| C | LUA-ORDER: the `SkillBuffs` / `SkillDebuffs` `modList` order. The buff names are sorted (Calcs.lua:761), but the mods of each `env.buffs[name]` / `env.debuffs[name]` come in PoB's per-process order. The control sorts these rows before display (CalcBreakdownControl.lua:337-345) | 73 entries (51 SkillBuffs, 22 SkillDebuffs) | Calcs.lua 755-805 (player), 844-860 (minion); PoB-vs-PoB §4: the order changes between PoB processes in 12 of 15 lists | compared as a multiset (the control's own sort) |
| D | PoB nondeterminism: an old-format gem (`skillId` and no `variantId`) takes the first `pairs(possibleVariants)` match with string keys. Data.lua gives the synthesised "Vaal Righteous Fire of Arcane Devotion" the base gem's `gameId` and `grantedEffectId`, so PoB picks either variant | 2 entries: `ReqStr` / `ReqInt` `rowList[2].sourceName` of upstream_Dual_Wield_Cospris_CoC (PoB "Vaal Righteous Fire of Arcane Devotion", Rust "Vaal Righteous Fire") | SkillsTab.lua:472-478; Data.lua:1127-1136; already noted in CCR-0104 ("PoB nondeterminism") | accepted name substitution (`POB_NONDET_NAMES`); registered as known_diffs `INT-3-1`. PoB-vs-PoB: in 1 of 4 PoB runs PoB picked Rust's variant (strict mode, `nd3`) |
| E | PoB nondeterminism: which flask's mod reaches the modDB. `mergeFlasks` walks `pairs(flasks)` (item-keyed, so address order) and `mergeBuff` keeps the first of two flasks with the same buff key | 5 queries: allflame_exp_000 `BleedImmune` / `CorruptedBloodImmune` (Iguana vs Sealing); allflame_exp_028 `SpellSuppressionChance` / `ChaosDegen` ×2 (two copies of Forbidden Taste, Item:6 vs Item:12) | CalcPerform.lua:1773, 45-67, 1731, 1758 | Tabulate rows compared with a flask's `Item:<id>:<name>` source replaced by `Item:<flask>` (`nondet_row`). Already recorded: known_diffs `PF-C-1`. PoB-vs-PoB: equal to Rust in 2 of 3 strict re-runs (000) and in 1 of 3 (028) |
| F | PoB nondeterminism: Explosive Trap and Explosive Trap of Shrapnel (`ExplosiveTrapAltX`) share one `statMap` table. Data.lua stamps the mods' `source` from whichever grantedEffect `pairs(data.skills)` (string keys) reaches last | 1 query in the first run (allflame_exp_019 `SmallExplosions`); 3 conversion queries in each of the 3 re-runs (`SkillPhysicalDamageConvertToFire`, BASE, cfg skill/weapon1/weapon2), with the source on the other side | act_dex.lua:7326 (the only `statMap = skills.X.statMap` at the pin); Data.lua:1049 and 1065-1079 | `Skill:ExplosiveTrapAltX` ≡ `Skill:ExplosiveTrap` in Tabulate rows (`nondet_row`); registered as known_diffs `INT-3-2` |
| G | LUA-ORDER: the Tabulate row order. This is the ModDB insertion order, which comes from string-keyed `pairs` (buffs, auras, curses), item-keyed `pairs` (flasks) and node-id maps whose insertion order comes from `pairs(tree.nodes)` of 3.10+ trees (the `root` string key, seeded hash) | 129 queries in 38 builds: rows equal as a multiset, different order | CCR-0024 §5, CCR-0028 §9, calc doc §3.4; e.g. allflame_exp_005 `PhysicalDamage` INC from Unnatural Instinct: the nodes come from `pairs(env.extraRadiusNodeList)` (CalcSetup.lua:284), which is filled from `pairs(node.nodesInRadius[…])` (CalcSetup.lua:898), and that is built from `pairs(self.nodes)` (PassiveTree.lua:670). A traced run showed PoB's and Rust's `nodesInRadius` orders for socket 31683 differ at position 3 | counted as "equal up to row order". PoB-vs-PoB §4: PoB's own row order changes between processes in 60 queries of the 8 re-run builds |

Totals of the first run: the 10 differing breakdown entries are 8 B (5 Physical, 1 each of Cold,
Fire and Lightning `MaximumHitTaken`) + 2 D. The 250 differing Tabulate queries are 244 H1 + 5 E
+ 1 F. Every entry and every query is equal once the harness fixes H1 and H2 are in, B and C are
compared as UI-sorted lists, and D-F are accepted as documented PoB choices.

## 4. PoB-vs-PoB (nondeterminism evidence)

`INT3_STRICT=1` runs of fresh PoB processes on two sets of builds. The first set
(allflame_exp_000, _019, _028, upstream_Dual_Wield_Cospris_CoC) got 3 more PoB runs. The second
set (allflame_exp_004, _005, _012, _015) got 2 more. Each PoB run went into a fresh cache.
Comparing PoB's own outputs across the runs (scratch script, PoB side only):

| build | PoB runs | SkillBuffs/Debuffs `modList` order varies | Tabulate queries whose rows vary (order only) |
|---|---|---|---|
| allflame_exp_000 | 4 | 2/2 | 12 (10) |
| allflame_exp_004 | 3 | 2/2 | 8 (8) |
| allflame_exp_005 | 3 | 2/2 | 2 (2) |
| allflame_exp_012 | 3 | 0/2 | 16 (16) |
| allflame_exp_015 | 3 | 1/2 | 2 (2) |
| allflame_exp_019 | 4 | 2/2 | 16 (12) |
| allflame_exp_028 | 4 | 1/1 | 3 (0) |
| upstream_Dual_Wield_Cospris_CoC | 4 | 2/2 | 10 (10) |

The non-order-only variations are exactly D, E and F: the flask item ids, the Explosive Trap
sources and the Vaal Righteous Fire variant. Against Rust, the set of order-only Tabulate queries
of the second set changed from run to run (19, 26 and 19 queries, with 7-8 entering or leaving).

One order is stable over the 3 PoB runs and still differs from Rust: allflame_exp_005's three
Unnatural Instinct `PhysicalDamage` rows. It comes from the 3.10+ `pairs(tree.nodes)` order,
which CCR-0024 §5 measured as per-process (4 Lua states, 4 orders). The traced
`nodesInRadius[31683]` orders differ between PoB and Rust (§3 G).

## 5. Fresh corpus (extra evidence, not the gate)

`INT3_CORPUS_DIR=<main>/.parity/fresh`: the 100 poe.ninja builds of `fresh-corpus.md`, with new
PoB runs.

* **First run.** Breakdown 19,205 / 19,209 entries equal. The 4 differing entries were all
  `SkillBuffs` (fresh_031, _038, _040, _089). Tabulate was 71,940 / 71,940.
* **The 4 entries (H3).** The difference was `modList[i].value.mod.tags`: PoB `[]`, Rust absent.
  These are `MinionModifier` rows (Transfusion and similar). The row's `value` is a LIST value
  `{ mod = … }` (Calcs.lua:765-770). state_dump.lua writes the inner mod as a mod, which always
  carries `tags`. The Rust export writes it through `plain_mod_json`, which leaves out an empty
  `tags`. Both mean the same empty tag list, so this is a representation gap and not a calc
  difference. Fixed in the harness: `norm` adds `"tags": []` to a mod-shaped object that has no
  `tags`.
* **Second run, with H3.** 0 PoB runs failed, 0 Rust errors.
  - Breakdown: **19,209 / 19,209** equal (19,006 exact in order).
  - Accepted: 118 `%d` casts and 877 closures.
  - Tabulate: **71,940 / 71,940** queries equal (343 equal up to row order, 6 equal up to PoB's
    nondeterministic sources), with 78,579 PoB rows.

The fresh run is not pinned, because the corpus is outside the repository.

## 6. Harness memory

A loaded Rust build is a reference cycle: `env.build` is a strong `BuildRef`, and
`build.calcsTab.mainEnv` / `calcsEnv` hold the envs. Without breaking that cycle the test leaked
about 20 MB per build-run. It peaked at 3.1 GB on all57 and at 4.9 GB on the fresh corpus.

The harness now breaks the cycle in `release()`, which clears the calcsTab handles before the
build is dropped. It also exports only the two breakdowns, not the whole env. With both changes
the peak is 1.48 GB on all57. A smaller per-build growth remains, about 13 MB per build-run, in
other calc-side cycles. `CA.rs` has the same `env.build` cycle, which may explain why the CA fresh
run was chunked (INT-1-fresh-triage.md §1).

## 7. Proposed follow-ups (not INT-3 scope; no product bug found)

* **ORC-X / probe** (optional): export Rust closure objects in calc tables (`SourceNameTooltip`)
  as `{"$fn": "<Lua file:lines>"}`, as `BREAKDOWN_FNS` already does for the breakdown module's
  closures. The harness would then compare the source instead of only presence (H2).
* **PT** (optional, already known): the 3.10+ `pairs(tree.nodes)` order cannot match PoB, because
  PoB's own order is per process (CCR-0024 §5). Rows tied on name and value in a Calcs-tab mod
  table can therefore show in a different order than in one PoB process. That is within PoB's own
  run-to-run variation.

No INT-3-B/C units are proposed: no bucket needs a change in a calc owner file.
