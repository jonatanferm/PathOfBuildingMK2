# Port follow-ups

Open items found while working on one unit that belong to another. Each entry names the owning
unit (calc doc §11.1 / ui doc §12), the evidence, and what closes it.

## CS0

Port review follow-ups: open issues the final review left on merged units.

1. `crates/pob-calc/src/modules/calc_setup/node_mods.rs:353-357` (port/CS0 @d08929d). In `buildModListForNode` L253, when the flag is off the port returns `Val::Bool(false)`, and `buildModListForNodeList` (L275, `if !explode.is_nil() { explodeSources.push(explode) }`) then pushes that `false` into `explodeSources`. Lua never does this. `ModListClass:FlagInternal` (Classes/ModList.lua:156-175, and the same in ModDB.lua:206-227) returns `true` or nothing, so `modList:Flag(nil, "CanExplode") and node` is the node or nil, and `t_insert(explodeSources, nil)` adds nothing. So a CanExplode mod whose Flag is false (e.g. one with an unmet tag, or after scaling or a jewel rewrite) gives Rust a non-empty explodeSources while Lua's stays empty. That flips CalcSetup.lua:1507 (`#env.explodeSources ~= 0`, which creates the 'On Kill Monster Explosion' group), and at 1530-1532 the code would index `.modSource` on a boolean. Fix: return `Val::Nil` in the off case. CCR-0074 §1 and §2 also need fixing: they promise CS2/CS3 that entries can be `Val::Bool(false)` and claim 't_insert(explodeSources, false) happens'. That is wrong and would lead the downstream units to handle a case that cannot occur. The oracle test can't catch this, because the corpus has no CanExplode mod whose flag is off. The fixer's uncommitted round-2 edits in worktree wf_daf28e03-290-29 make exactly this change; they have to be committed (and the CS0 test rerun) before the unit is accepted.
   - Integrator note: at merge time those round-2 edits were still uncommitted in `.claude/worktrees/wf_daf28e03-290-29` (node_mods.rs, support.rs `gemIsTypeVal` non-string keyword handling, tests/CS0.rs, CCR-0074). They are not part of this merge.

## HYG2 exposed

HYG2 (test audit fixes V1, V2, R1) made the oracle tests in `crates/pob-oracle/tests` fail on a
missing input and bounded their tolerated buckets. Measured on `rust-port` 746b103 (2026-11-09).
Where a newly honest bound records a real gap, the bound is the measured value and points here,
so the suite cannot silently get worse; lowering the bound is part of closing the item.

### Gaps behind a bound set to the measured value

- **u0_smoke: the CALCS runs raise on the PF-B gap** (owner: PF-B). Was "buildOutput leaves the
  player output empty"; resolved by CA (port/CA, 2026-11-10): the 10 MAIN / CALCULATOR runs now
  leave 464-698 player output keys, and `tests/u0_smoke.rs` requires a non-empty output for every
  run that completes. The 5 CALCS runs raise `attempt to compare number with nil` (LuaJIT's text) at
  Calcs.lua:605 (`output.PowerCharges > 0`): `PowerCharges` is nil because `doActorCharges`
  (CalcPerform.lua:980-1139) is still a stub. The test tolerates a Lua error only on a run that
  hits a stub, bounded by `MAX_STUB_ERRORS = 5`; lower it to 0 when PF-B lands. Still hit on
  slice5: `CalcSetup.lua:800-1450` (CS2), `CalcPerform.lua:636-1272` (PF-B) and
  `CalcsTab.lua:441-472` (PWR, called by `loadBuildFromXML`). u0_smoke now also registers the
  timeless-jewel LUT file system (slice5_allflame_exp_030 failed to load without it since PS-B).

### Pre-existing failures seen during the HYG2 runs (not caused by HYG2's assertions)

These assertions were already in the files and fail on 746b103 before any HYG2 bound is reached.

- **BM-B `corpus_tabs_match_oracle` and `synthetic_sections_match_oracle`** (owners: CO-A /
  BM-B). All 57 corpus builds and all 3 synthetic files fail in `config Load` with
  `attempt to index field '?' (a nil value)`, raised by
  `crates/pob-calc/src/modules/config_options/apply_a.rs:5217` (`opt_254_apply`,
  `varControls['enemyRadius']:SetPlaceholder`, CCR-0037: a missing control raises). The build
  model the BM-B test loads has no `enemyRadius` control. Because of this, the HYG2 floors in
  `tests/BM-B.rs` (`TABS_BOUNDS` / `BUFFS_BOUNDS` / `SYNTHETIC_BOUNDS`) come from the last green
  runs on rust-port (148,776 / 3310 / 1302 values compared, 0 / 5 / 0 pending), not from 746b103.

### Bounds that are PoB's own nondeterminism

Not port gaps, recorded so a reader knows why they are not 0:

- AS `LUA-ORDER` (stat-map mods added in `pairs` order, calc doc §3.4/§9.5) and AltX/AltY
  sources (CCR-0010); CT `stat_map` PoB-nondeterministic records; PS-A `leap`
  (order-dependent, asserted as a set under `leapset`). The bounds are the observed maxima plus
  headroom, stated next to each constant.
- BM-B `LUA-ORDER`: not bounded by an observed count (LuaJIT randomises `pairs` per process,
  and a count with headroom went red at random: tabs 12 -> 18/19 on the same code). Flipped
  ambiguous gems are bounded by the ambiguous gems in the oracle's Saves (per build and in
  total), reordered party labels by the multi-line labels compared; only the deterministic
  `expected` bucket has a fixed bound.

Not port gaps either, but limits of a test's own rebuilt model:

- **MS-A: 6771 of 1,326,872 tag-path queries differ** (`MS_A_BOUNDS.max_tag_path_misses = 6771`,
  `tests/MS-A.rs`; unchanged after the MS-B fix round, re-measured 2026-11-10). MS-A's Rust model
  rebuilds only the stores, actor outputs and cfg flags/keywordFlags/source/skillName/skillStats;
  the actor links, `itemList` items, minionData, weaponData and the cfg fields EvalMod reads
  (skillCond, skillTypes, skillGrantedEffect, slotName, item, ...) are absent, so tag paths that
  read them answer differently. MS-B's `l3_full_matrix_all57` rebuilds all of them and gates the
  same tag paths at 100% (6,948,606/6,948,606 matrix and 5,668,313/5,668,313 synthetic queries).
  The bound only keeps MS-A's ungated count from drifting.

## BM-A

Port review follow-ups: open issues the final review left on merged units.

1. Integration with rust-port broke the BM-A acceptance test. PS-B (`BuildAllDependsAndPaths`, 17bdabd) was merged on rust-port as 4733437, and port/BM-A did not contain it. After merging rust-port into port/BM-A, `POB_ORACLE_REQUIRE=1 scripts/port/cargo.sh test -p pob-oracle --test BM-A -- bm_a_corpus` loaded only 23 of 57 builds in Rust. The other 34 raised `Rust Error occurred loading Timeless Jewel data` or `... Glorious Vanity data` (pob-gamedata/src/timeless_lut.rs:1012/1022), because `bm_a_corpus` in crates/pob-oracle/tests/BM-A.rs never registered a timeless-jewel file system. The default `StdJewelFs::new("")` probes `./` and `./src` from the crate directory and finds no data.
   - Integrator note: fixed before this merge. port/BM-A now merges rust-port (5997856) and has ba622fa, which registers `timeless_lut::set_jewel_fs(StdJewelFs { script_path: canonicalize(src), writable: false })` and `set_con_printf(|_| {})` in `bm_a_corpus` (the same as PS-B.rs), and calls `clear_timeless_jewel_luts(None)` after each `run_build`. The acceptance test was re-run on the merged rust-port tree; the result is recorded in the merge report.

## CA

Open items found while porting Calcs.lua (port/CA, 2026-11-10; `crates/pob-oracle/tests/CA.rs`).

1. **L6 (end to end) is not met; blocked on CS2 and PF-B** (owners: CS2, PF-B). `ca_l6_corpus`
   on all57: headline keys 195/741 exact (26.3 %), 0 builds with every headline key exact, all
   main-output keys 21,822/47,610 exact (45.8 %), CALCS lists 0/171, main-env used-tracking key
   sets 174/513, misc calculator headline values 156/1248. Cause: `initEnv`'s item segment
   (CalcSetup.lua:800-1450, CS2) is a stub, so no item mod reaches the player (e.g. Life ~1245,
   resists at their -60/-45 base on most builds), and every build's CALCS `buildOutput` raises at
   Calcs.lua:605 because PF-B's `doActorCharges` leaves `output.PowerCharges` nil (57/57; the
   test catches the CALCS stage and still compares MAIN). The test's floors
   (`L6_MIN_HEADLINE_EXACT = 195`, `L6_MIN_KEYS_EXACT = 21_822`, `L6_MAX_RUST_ERRORS = 57`) are
   the measured values; raise them as CS2 / PF-B land, and make headline exact a hard gate then.
   The error bound is also split per stage (`L6_MAX_STAGE_ERRORS`: CALCS 57, every other stage,
   including a raising misc calculator call, 0). `ca_d13_fixed_point` covers MAIN mode only; add
   CALCS once PF-B lands (it raises at Calcs.lua:605 today).
   CA's own blocks are verified on PoB's injected state by `ca_l5_post` (all clean).
   **Re-measured at the CA merge (with PF-B on rust-port, 2026-11-10):** headline 194/741, all
   keys 24,381/48,958, CALCS lists 78/171, used-tracking 183/513, misc 154/1248, 0 Rust errors
   (PF-B's `doActorCharges` fixed the Calcs.lua:605 raise). Floors now 194 / 24,381 / 0 errors
   (no stage allowed). The one headline key lost vs port/CA alone is allflame_exp_002 `TotalDPS`
   (PoB 0; 0 under the stubs, 0.598752 now). The merge also fixed a PF-B bug the corpus exposed:
   `initMinionModDB` indexed `minion.extra["lifeTable"]` as a table, but CalcActiveSkill stores a
   `LifeTable` object (CCR-0040), so every build with a minion raised "attempt to index a userdata
   value" (CalcPerform.lua:1160); `minion_lifeTable` (calc_perform/actor_b.rs) now converts it.
   PF-B's own test injects tables and did not see it.
   **Changed by CS2 in `CA.rs` (port/CS2, 2026-11-10; for the CA owner to review):** L6 floors
   raised to the values measured with CS2's items in the env (`L6_MIN_HEADLINE_EXACT = 728`,
   `L6_MIN_KEYS_EXACT = 44_908`, `L6_MAX_RUST_ERRORS = 4`, `L6_MAX_STAGE_ERRORS` MAIN / CALCS /
   getMiscCalculator / configTab GetMiscCalculator 1 each: the upstream_Dual_Savior mirage raise of item 6),
   `D13_MAX_ERRORS = 1` (the same raise), and `D13_POB_WARM_CACHE` (item 5, measured in PoB).
   **Closed by INT-1-B (port/INT-1-B on rust-port 4870179, 2026-11-11).** With INT-1-A
   (CCR-0113), INT-1-C and INT-1-B's `flat_table` marker rendering, the full corpus
   (`POB_FULL_SUITE=1`) is exact: 57 builds, headline 741/741, all keys 45,954/45,954, CALCS
   lists 171/171, used-tracking 513/513, misc 1,248/1,248, 0 Rust errors. The floors and the
   per-stage error bounds are gone: `ca_l6_corpus` now asserts every compared value exact and no
   Rust error on any corpus subset, and pins the full corpus's totals (`L6_BUILDS`, …,
   `L6_MISC`). `ca_d13_fixed_point` now covers CALCS mode too (two CALCS runs with
   `calcsTab.mainEnv` set): 62/62 fixed points, 0 errors, no error bound. Details in
   docs/port/parity/INT-1-triage.md §8.
2. **`CalcsTab:BuildOutput` scaffold in CA.rs** (owner: PWR). `power::BuildOutput`
   (CalcsTab.lua:441-472) is still a stub, so `loadBuildFromXML` computes nothing; CA.rs carries
   a scaffold copy (`calcsTab_BuildOutput`, `rust_load`: Init's pass plus PoB's `OnFrame`
   rebuilds, as many as PoB made per the probe counts). PWR replaces it with the real function.
3. **Injected items carry no mod lines** (owner: ORC-X). `pob-state/1` item dumps hold
   modList/slotModList/…, not the `*ModLines` that `Item:FindModifierSubstring` reads, so an
   `ItemCondition` tag with `searchCond` evaluates differently on an injected env. Seen in
   `ca_l5_post` on allflame_exp_016 / _049 (`Spec:LifeInc` sums Tree:41415, "+15 % Life if no
   Life on body armour"); counted as harness-limited (`L5_MAX_LIMITED`).
4. **Skills PoB leaves uncached** (information; investigated in fix round 1). On
   upstream_Dual_Savior PoB never has a `GlobalCache.cachedData.MAIN` entry for its two Reflection
   skills (`Reflection_WEAPON1_1_3` / `_WEAPON2_1_4`). `buildActiveSkill` finds the uuid and runs
   `calcs.perform`, but The Saviour's mirage `postCalcFunc` (CalcMirages.lua:154-159) makes the
   mirrored attack `env.player.mainSkill`, so `cacheData(cacheSkillUUID(env.player.mainSkill, env),
   env)` (CalcPerform.lua:3989) files the result under that skill's uuid (`DoubleStrike_WEAPON1_1_1`,
   measured in PoB). The Rust loop on an injected state would therefore call `buildActiveSkill`
   (the full calc on a placeholder build), so this build stays harness-limited in `ca_l5_post`
   (cost-warning block only; `L5_MAX_LIMITED[0] = 1`) and is covered only end to end. The other two
   builds listed earlier (upstream_Dual_Wield_Cospris_CoC, upstream_Generals_Perforate_Zerker)
   were a harness gap, not PoB behavior. PoB caches those skills during the loop
   (`IceSpear_WEAPON2_1_6`, `CommandmentofFrost_GLOVES_1_8`, `Dash_WEAPON1_1_2`,
   `Portal_WEAPON1_3_2`), so the `ca:399` dump carries no `$uuid` for them and the rebuilt
   group list missed their groups. `fill_socket_groups` now takes their uuids from PoB's final
   cache keys (unique `name_SLOT_slotIndx_` prefix match), and both builds' cost warnings are
   compared clean.
5. **D13 does not hold in PoB on a warm GlobalCache for allflame_exp_050** (owner: CA;
   information, found at the CS2 merge, 2026-11-10). With CS2's items in the env,
   `ca_d13_fixed_point`'s second `buildOutput` on allflame_exp_050 differs from the first
   (BleedStacks 0.5875 → 1.25, CombinedDPS 7,411,955.92 → 15,755,502.09). PoB does the same:
   `pob_headless` (POB_ENGINE=jit) on rust-port d8a4407, `wipeGlobalCache()`, then
   `calcs.buildOutput(build, "MAIN")` twice, gives 26 differing output keys out of 261,774, with
   the same values (BleedStacks 1.25, CombinedDPS 15,755,502.093183, LifeLeechInstances 2.35 → 5,
   MainHand.BleedDotMulti 1.35 → 1.36085…). With a `wipeGlobalCache()` between the runs, 0 keys
   differ in PoB and in Rust, and the first run equals PoB's L6 output. So this is PoB behavior
   (a GlobalCache entry the first run leaves feeds the second run's bleed and leech stacks), and
   the Rust port reproduces it. `CA.rs` records it as `D13_POB_WARM_CACHE`: for that build the
   warm run is checked against PoB's warm values and the fixed point is checked after a wipe.
   A new build that differs on a warm cache must first be measured in PoB the same way.
   **Second build: fresh_017** (INT-1-FRESH corpus, `.parity/fresh`; found by INT-1-FRESH,
   docs/port/parity/INT-1-fresh-triage.md §4 B; recorded by INT-1F-B, 2026-11-12). An SSF
   Allflame Champion, Lacerate of Butchering, also a bleed build. Measured the same way in PoB
   (`pob_headless`, POB_ENGINE=jit, PoB v2.67.2): 24 of 78,992 output keys differ between the
   two runs (BleedStacks 0.675 → 1.25, LifeLeechInstances and ManaLeechInstances 2.7 → 5,
   BleedDPS 34,158.45 → 53,560.428711765730, TotalDPS 2,277,409.28 → 4,217,424.593125107,
   CombinedDPS 2,311,567.73 → 4,270,985.021836872), and 0 differ with a wipe between them.
   Rust's warm run has PoB's warm values, and L6 (the cold run) is exact for this build. It is
   the second entry of `D13_POB_WARM_CACHE`.
6. **The Saviour's Reflection mirage skill outlives its env's actor** (owners: MI and AS;
   found at the CS2 merge, CCR-0105 §6). upstream_Dual_Savior raises `attempt to index field
   'actor' (a nil value)` (calc_active_skill.rs:713) in MAIN, CALCS, `getMiscCalculator` and
   `configTab GetMiscCalculator`. The mirage's `postCalcFunc` sets `env.player.mainSkill =
   newSkill` (CalcMirages.lua:155). `newSkill` belongs to the mirage env, which is then dropped,
   and `ActiveSkill.actor` is a `Weak` back-pointer (calc doc §3.3 rule 4), so the skill's actor
   is gone. Lua keeps the actor alive through the skill. CALCS then has no `mainEnv`. The four
   errors are what `L6_MAX_RUST_ERRORS = 4` / `L6_MAX_STAGE_ERRORS` (one per stage) and
   `D13_MAX_ERRORS = 1` in `CA.rs` allow. Closed when MI keeps the mirage env (or its player)
   alive for as long as `env.player.mainSkill` points into it, e.g. a strong reference held by
   the outer env, as CCR-0105 §4 does for `override.extraJewelFuncs.actor`. After that, lower
   the bounds to 0.
   **Cause corrected, and closed (INT-1, 2026-11-11).** The actor that dies is not the mirage
   env's but the **source** skill's: CalcMirages.lua:40 copies DoubleStrike from an earlier
   `buildActiveSkill` env, and CalcPerform.lua:3989's `cacheData` then overwrites the
   GlobalCache entry that was that env's only owner, so the copy's `Weak` actor dies
   (docs/port/parity/INT-1-triage.md §4 A). Keeping the mirage env alive (MI) would not help.
   Fixed in AS by INT-1-A: `copyActiveSkill` keeps a foreign actor alive on the copy
   (CCR-0113, `0113-int-1-a-foreign-actor-keepalive.md`). INT-1-C made the MAIN env honour
   `calcs.mirages`' return (CalcPerform.lua:3798). The bounds are now 0 (INT-1-B): `CA.rs` has
   no `L6_MAX_RUST_ERRORS` / `L6_MAX_STAGE_ERRORS` / `D13_MAX_ERRORS` any more, and the tests
   assert no error; upstream_Dual_Savior is 903/903 at L6 and a D13 fixed point.

## CS2

Open items found while porting initEnv's item segment (port/CS2, 2026-11-10;
`crates/pob-oracle/tests/CS2.rs`, CCR-0105).

1. **Nothing writes the node-id half of `override.removeNodes` yet** (owner: U24, information
   for PWR). CalcSetup.lua 1393/1408 test `override.removeNodes[node.id]`. In PoB only the
   anointed-node tooltip path (PassiveTreeView.lua:1706, U24's AddNodeTooltip) builds that
   id-keyed set. The typed `Override.removeNodes` is node-keyed (CS1, CCR-0082 §2), so CS2
   reads the id keys from `override.extra["$removeNodeIds"]` (a table id → true; CCR-0105 §1).
   The unit that ports PassiveTreeView.lua:1706 must fill that table, not `removeNodes`. PWR's
   own calls (CalcsTab.lua 626/638) are node-keyed and need no change.
2. **Dumped jewel closures lose their upvalues on injection** (owner: ORC-X; information).
   `{"$fn": src}` decodes to a `JewelFuncSpec` with empty `args` (mod_json.rs, `lua/state_dump.lua`
   cannot read upvalue values). An injected `ExtraJewelFunc` mod whose closure reads them
   (ModParser.lua:2630-2634 `firstToUpper(attr)`) raises when it runs. `CS2.rs` restores them
   from a second, never-injected load of the same build (`restore_jewel_func_args`). Any other
   test that runs an injected ExtraJewelFunc or `jewelData.funcList` closure with captures needs
   the same workaround, or a dump that carries the captures.
3. **Two explained buckets of `cs2_l4_items_segment` rest on exporter gaps** (owner: ORC-X2).
   `override.extraJewelFuncs (second pass, $foreign actor)`: on a restart's recursive call the
   Rust store is pass 1's (CalcSetup.lua:1425 leaves an existing store untouched), whose actor
   the exporter writes as a `$foreign` object, so its paths cannot be compared with PoB's
   dump. `radiusJewelList.nodes`: the node-id map is exported as userdata, so the test compares
   the id sets itself (`radius_nodes_equal`). Both are bounded in `CS2.rs`
   (`EXPLAINED_MAX`, the measured counts) so a regression cannot move more hits into them
   silently; close the gaps in the exporter and drop the buckets. The `pre-load` skip count is
   bounded the same way (`PRELOAD_MAX`).

## CCR2

Open issues from the CCR2 final review (owner: whoever next touches scripts/port tooling).

1. Commit a7f2683 added 13 Python bytecode files to git:
   `scripts/port/__pycache__/{calcscope,globals_lint,locals_audit,luaast,pairs_inventory,pobpaths,provenance,segments}.cpython-314.pyc`
   and `scripts/port/tests/__pycache__/{_util,test_lint_inventory,test_locals_segments,test_luaast,test_provenance}.cpython-314.pyc`.
   rust-port tracked no `__pycache__` files before. They are build artifacts tied to one Python
   version that record the worktree path of the run that made them, so running the scripts in any
   other checkout dirties the tree (confirmed in a fresh port/CCR2 worktree after running
   pairs_inventory/provenance/segments: ` M scripts/port/__pycache__/{calcscope,luaast,pobpaths,segments}.cpython-314.pyc`).
   Fix: `git rm -r --cached scripts/port/__pycache__ scripts/port/tests/__pycache__` and ignore
   `__pycache__/` / `*.pyc`.
   **Resolved at integration** in c477ed9 (files untracked, `.gitignore` now ignores `__pycache__/`
   and `*.pyc`). Units branched from port/CCR2 before c477ed9 still carry the tracked files and
   pick up the removal on merge; do not re-add them.

## FUP1

Found while running the touched units' acceptance tests on port/FUP1 (rust-port 85604a9 + FUP1,
2026-11-10).

1. **PF-C `pfc_l4_perform_1289_1943` fails: 55/57 builds clean** (owner: PF-C; dumps probed
   fresh with `corpus/PF-C/probe.sh`). Identical with PF-C's own `buff_list_order_only` (the
   pre-FUP1 test, run from a temporary copy) and with the shared
   `testkit::diff_against_normalised` (55/57, 191 LUA-ORDER-only, 672 model-only both times), so
   it is not caused by FUP1's move. `allflame_exp_039`: the segment raises `attempt to index a
   userdata value` (the shape CA's merge fixed for `minion.extra.lifeTable`, CCR-0040; look for
   a replayed callee that stores a `LifeTable` object where the segment indexes a table).
   `allflame_exp_000`: 4 mismatches, `env.modDB.mods.BleedImmune` / `CorruptedBloodImmune` come
   from flask `Item:20` (Flagellant's Basalt Flask of the Iguana) in Rust but from `Item:23`
   (Seething Divine Life Flask of Sealing) in PoB, i.e. which of two flasks granting the same
   immunity wins.
2. **DF-E all57 was red on mixed-run dumps, not on the calc** (resolved in FUP1, recorded for
   the record). The 139 failing hits of 363 (fullsuite 2026-11-10 02:34: 171/363) paired an
   `:in` and an `:out` dump of different PoB processes found in `.parity/probe-cache/<sha>/<build-key>/`
   (or a stale private `target/dfe-probe`), whose string-keyed `pairs` orders differ
   (`env.auxSkillList[].baseSkillModList`, `env.buffs.*`, buff mod lists: state the segment never
   touches). `tests/DF-E.rs` now takes its dumps from its own selection entry of the shared probe
   cache (one run per build): 363/363.
3. **`DIVERGENCE(calc-item-errors)` can go** (owner: U16, `crates/pob-app/src/main_mode/init.rs`
   `guard_item`). Its one known case, `rare#195`'s Craft raising "non-integral number keys are
   not supported by Table (929.1)", is fixed: `Table` takes float keys (CCR-0102 §1) and IT-B's
   Craft is 207/207. Remove the guard (and the console line) once nothing else needs it.
4. **Patched-src prune can pull a copy from under a long run** (owner: whoever next touches
   `crates/pob-oracle/src/patcher.rs` `prune`). A `<.parity>/patched-src/<sha>` copy whose
   `.last-used` is older than 24 h (`POB_PATCHED_SRC_MAX_AGE_HOURS`) is renamed away even while
   another process that took it more than 24 h earlier still loads Lua files from it; `keep`
   protects only the pruning process's own sha. Marker-less legacy copies in `target/oracle-src`
   go after 72 h the same way, under an older branch still using them. Unlikely at current run
   lengths; closing it means holding the marker for a run (re-touch it while a process holds the
   copy, or a per-process lock file the prune checks).
5. **`runner::shared_patch_root` doc is stale** (owner: the next change to
   `crates/pob-oracle/src/runner.rs` for another reason). It still says "every worktree shares
   one copy per sha" of `target/oracle-src`; since FUP1 `patcher::patch` publishes into
   `patcher::shared_root()` whatever root it is given. Left alone in FUP1 because `runner.rs` is
   `include_str!`'d into `PROBE_RUNTIME`, so any edit retires every cached probe selection.
6. **NaN `f64` keys still raise on reads outside item.rs / item_parse.rs** (owner: the units of
   each file; predates FUP1). Helpers such as `val_key` in `calc_active_skill.rs`,
   `init_env_skills.rs`, `item_mods.rs` and `passive_tree.rs` build the read key with
   `Key::from(f64)`, which raises on NaN, where Lua's `t[0/0]` reads nil. FUP1 switched only
   item.rs / item_parse.rs to `try_num`; the others should read through it too.

## PF-B

Final-review open issues, both resolved in the merge commit's follow-up (listed for the record):

1. **CCR number collision (resolved).** PF-B's CCR was `0091-pf-b-minion-fields-and-fn-probes.md`,
   but 0091 is CT's on rust-port (AMENDMENTS A8). Renamed to
   `docs/port/ccr/0095-pf-b-minion-fields-and-fn-probes.md`; every `CCR-0091` marker in
   `crates/pob-calc/src/modules/calc_perform/actor_b.rs` (8) and `crates/pob-oracle/tests/PF-B.rs`
   (2) now reads `CCR-0095`. **Still open (U16 / CA / CCR3):** U16 merged with
   `docs/port/ccr/0096-u16-main-mode-seams.md` without renumbering, so 0091 is still held twice on
   rust-port (CT and U16). Per A8, U16 must take the next free number (0096 now that PF-B holds
   0095) and update its markers; CA's in-flight `0091-ca-calcs-seams.md` takes the one after (renamed to `0097-ca-calcs-seams.md` at the CA merge, leaving 0096 for U16).
2. **Binding doc out of date with the code (resolved).** PF-B's fix rounds changed the
   `applyEnemyModifiers` port in `calc_perform/actor_a.rs` (`resource_payload` rebuilds the
   actor's `EnemyModifier` mods with the re-sourced payload; `enemy_cache::set` keys both the
   original and the re-sourced `Arc`) and rewrote §3 of the already-applied CCR-0085.
   contracts.md#mod-mutation row "CalcPerform.lua 629" now states the payload swap, the
   two-identity cache key, the pass-2 `source` difference on `env.modDB`/tree/config/item lists
   and the replaced outer `Arc`; CCR-0085 §3 carries a dated "revised by PF-B" note and CCR-0095
   §4 cites the row. Owner of any further change: whoever next touches `applyEnemyModifiers`
   (a unit that keys on the outer `EnemyModifier` `Arc` files a CCR).

## PWR

Open items found while porting the power report (port/PWR, 2026-11-10; `crates/pob-oracle/tests/PWR.rs`,
CCR-0101).

1. **CA.rs still carries its `calcsTab_BuildOutput` scaffold** (owner: CA). `power::BuildOutput`
   (CalcsTab.lua:441-472) is ported, so `loadBuildFromXML` now runs MAIN, CALCS and the misc
   calculator. The scaffold catches the CALCS stage on its own, which the real function does not
   (an error there aborts the call, as in PoB); CA can switch when it no longer needs that.
2. **Jewel-closure modKeys collide in the port** (owner: CT, `mod_tools.rs` `FUNCTION_TEXT`;
   information). `ModTools.formatValue` prints a jewel `func` as `function: builtin`; PoB prints
   the closure address. The power report keys its cache by `node.modKey` (CalcsTab.lua:613), so
   two nodes whose mods differ only in their jewel closure (same `type`/`radius`) share one cache
   entry in Rust and not in PoB. Not seen in the PWR builds (PWR.rs normalises the address when it
   matches PoB's calls; node 50692 is the one such node there); would show as a power difference
   and a call-count difference in `pwr_l5_replay`.
3. **End-to-end node power is not at parity** (owners: CS2 and the calc units behind CA's L6
   floors). `pwr_l6_report`: upstream_OccVortex Offence/Defence, all nodes, real Rust calculator
   vs PoB 6452/7676 power values exact (Rust defence 0 where PoB has item-dependent defence);
   PWR itself is exact on PoB's calc results (`pwr_l5_replay`, 108,820/108,820). Gate it once
   CA's L6 headline is exact.
4. **U26 drives the report** (owner: U26). `power::BuildPower` is the per-frame resume
   (PassiveTreeView.lua:785); TreeTab state goes through the CCR-0101 accessors
   (`set_powerStat`, `set_nodePowerMaxDepth`, `set_powerBuilderCallback`,
   `set_powerBuilderProgressCallback`, `powerMax`); `set_launch_devMode` and `set_GetTime` come
   from U16's launch state / the frame clock.

## INT-1F-A

1. **INT-1F-B's fresh-corpus pin comment goes stale once both merge** (owner: whichever of
   INT-1F-A / INT-1F-B merges second; INT-1F-B was not on rust-port when INT-1F-A finished). The
   pin value needs no bump: INT-1F-B's `L6_FRESH.keys = 85_603` is what `ca_l6_corpus` compares
   after INT-1F-A (the 3 Rust-only `ManaCostWarningList` keys are gone). Its doc comment ("Until
   INT-1F-A lands, Rust adds 3 Rust-only … keys … fails the exactness assert") should then say
   that INT-1F-A removed them (CCR-0133). **Done** at the INT-1F-A merge (INT-1F-B merged first).
2. **CalcPerform.lua:1985-1991 still ignores `levelCostCleared`** (owner: PF,
   `calc_perform/pools_charges.rs` `has_cost` / `cost_of`; AMENDMENTS A8 item 11). A build with
   `ManaCostGainAsReservation` / `LifeCostGainAsReservation` and an item-granted triggered skill
   would add the level's cost to `pool.<res>.baseFlat`, where PoB adds 0 (the emptied table is
   still truthy, so PoB takes the branch with `cost.Mana or 0`). Not seen in either corpus.
   INT-1F-A fixed only the OF-C reader (CalcOffence.lua:1720).
