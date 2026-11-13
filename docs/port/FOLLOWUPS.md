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
  **Closed by FUP2** (port/FUP2 on rust-port 4c24bc2, 2026-11-12): 0 of the 15 runs raise and no
  orchestrator stub is hit (0 distinct unported sites); `MAX_STUB_ERRORS` is 0.

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
  The bound only keeps MS-A's ungated count from drifting. (FUP3 left it alone: MS-A's
  stores come from its own `corpus/MS-A/l3_queries.lua` serialisation, not from pob-state dumps,
  so FUP3's `$modLines` / `$up` do not reach it.)

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
   **Closed by FUP2** (port/FUP2, 2026-11-12): CA.rs's `calcsTab_BuildOutput` now calls
   `power::BuildOutput` (an error aborts the call, as in PoB). The CALCS-stage catch is no longer
   needed: no corpus build raises in any stage. With it, `POB_FULL_SUITE=1`: `ca_l6_corpus` all57
   headline 741/741, all keys 45,954/45,954, CALCS lists 171/171, used-tracking 513/513, misc
   1,248/1,248, 0 Rust errors; `ca_d13_fixed_point` 62/62 fixed points; `ca_l5_post` clean.
   `CA_CORPUS_DIR=.parity/fresh`: D13 100/100 fixed points, L6 0 Rust errors, headline
   1,300/1,300, CALCS lists 300/300, used-tracking 900/900, misc 2,200/2,200, but all keys
   85,602/85,603 and `ca_l5_post` 4 builds failing; both are the same on rust-port 4c24bc2
   without FUP2 (see "FUP2" below), so they are not caused by the switch.
3. **Injected items carry no mod lines** (owner: ORC-X). `pob-state/1` item dumps hold
   modList/slotModList/…, not the `*ModLines` that `Item:FindModifierSubstring` reads, so an
   `ItemCondition` tag with `searchCond` evaluates differently on an injected env. Seen in
   `ca_l5_post` on allflame_exp_016 / _049 (`Spec:LifeInc` sums Tree:41415, "+15 % Life if no
   Life on body armour"); counted as harness-limited (`L5_MAX_LIMITED`).
   **Closed by FUP3** (port/FUP3, 2026-11-12). The dump format is now `pob-state/2`
   (`schema::STATE_FORMAT`): every item carries `"$modLines"` (`lua/state_dump.lua`
   `itemModLines`: the explicit / enchant / scourge / implicit / crucible lines with the fields
   FindModifierSubstring and CheckModLineVariant read, Item.lua:289-333/2120-2146, and the item's
   variant selection fields), and `inject::fill_mod_lines` sets the injected item's `*ModLines`,
   `variantAlt*` and `extra` from it. `ca_l5_post` no longer excuses `Spec:*` sums over
   searchCond Tree mods (the `limited` branch of `main_block` is gone) and `L5_MAX_LIMITED` is
   `[1, 0, 0]`: allflame_exp_016 and _049 are compared clean. `CA.rs`'s private
   `target/tmp/ca-probe/<stem>` dumps carry a `format.txt` and are re-probed when older.
   `$modLines` is an injection input, ignored by every comparison (`schema::INPUT_KEYS`).
   The same gap was PF-A's four CCR-0085 §5 `KNOWN_DIFFS` (allflame_exp_016 / _049,
   doActorLifeMana and doActorAttribsConditions): all four came out clean (stale) on pob-state/2
   dumps, so `PF-A.rs`'s list is now empty (its `target/tmp/pfa-probe` dumps carry a
   `format.txt` too).
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
   **Closed by FUP3** (port/FUP3, 2026-11-12). The probe host gives the encoder an upvalue
   reader (`JitPob::install_probe_upvalue_reader`, a `lua_getupvalue` C function stored in
   `__state_dump`, not a global `debug`, so PoB's environment is unchanged; called by
   `runner::probe_in_process`), and a ModParser.lua closure is dumped as
   `{"$fn": src, "$up": [[name, value], …]}` (pob-state/2; one level: functions among the values
   carry no `$up`). `mod_json::closure_args` fills `JewelFuncSpec.args` for the closures whose key
   is their source range (`CLOSURE_ARGS`: 2622-2627, 2630-2634 and the jewelOtherFuncs inner
   closures 6232-6364). `restore_jewel_func_args` is gone from `CS2.rs`; `check_jewel_func_args`
   now checks the decoded upvalues against a fresh parse (all57 + corpus/CS2 with
   `POB_FULL_SUITE=1`: 93 injected closures checked, 0 differ). Still not covered: the
   jewelFuncList wrappers (6564-6569 and the threshold-table wrapper), whose Rust key is the
   jewelFuncList key and not the closure's range; their `$up` is dumped but not decoded.
   In-process probe preludes that boot their own `JitPob` (CS1, INT-3, CA's `pob_run`, the UI
   oracle) do not install the reader, so their dumps have no `$up`.
3. **Two explained buckets of `cs2_l4_items_segment` rest on exporter gaps** (owner: ORC-X2).
   `override.extraJewelFuncs (second pass, $foreign actor)`: on a restart's recursive call the
   Rust store is pass 1's (CalcSetup.lua:1425 leaves an existing store untouched), whose actor
   the exporter writes as a `$foreign` object, so its paths cannot be compared with PoB's
   dump. `radiusJewelList.nodes`: the node-id map is exported as userdata, so the test compares
   the id sets itself (`radius_nodes_equal`). Both are bounded in `CS2.rs`
   (`EXPLAINED_MAX`, the measured counts) so a regression cannot move more hits into them
   silently; close the gaps in the exporter and drop the buckets. The `pre-load` skip count is
   bounded the same way (`PRELOAD_MAX`).
   **FUP3 (2026-11-12): not closed, bounds unchanged.** Both gaps are in the Rust exporter
   (`crates/pob-calc-core/src/probe.rs`), outside FUP3's crate. `radiusJewelList.nodes` is a bare
   `Val::Obj(Rc<LjIntMap<NodeRef>>)` (CCR-0024 §3) that `PObj::from_val` only knows boxed, so it
   exports as `{"$userdata": true}`; the second-pass actor needs a change to the export
   identity rules. Both are proposed in CCR-0140 (`0140-fup3-exporter-gaps.md`). Re-measured
   with `POB_FULL_SUITE=1` on port/FUP3: 17,414 and 129,887, exactly the bounds; `pre-load` 354.

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
   **Resolved in PFC1** (rust-port 64d1e45 + PFC1, 2026-11-12): 57/57 on the full corpus, 14/14
   on the quick corpus.
   - The `attempt to index a userdata value` raise no longer reproduces on 64d1e45 (not
     bisected; RETYPE db07985 / c626f6c replaced the `minion.extra` `LifeTable` object with the
     typed `Actor.lifeTable`, which `minion_lifeTable` reads as a data table). What was left for 039
     was `Missing env.minion.output` plus `locals.output.Minion` (PoB: a `$ref` to it). The cause was
     probe bookkeeping in the real `initMinionModDB`: L1157 `minion.output = output or { }` did
     not clear the injected "absent" mark (CCR-0079 §5), so the still-empty table was exported
     as nil. PF-B's L3 test cleared that mark in its harness (`tests/PF-B.rs` "inject marks a
     field PoB had as nil"); the calc now clears it itself (`actor_b.rs` `probe_written`, a
     no-op without the `probe` feature).
   - 000's flask source is PoB nondeterminism that the harness could not see. `env.flasks` is
     keyed by item tables, so `pairs(flasks)` (CalcPerform.lua:1773) follows addresses. Three
     fresh probes of 000 visited items 23,20,19,10,15 / 15,10,19,20,23 / 23,19,15,10,20, and
     the first flask merged wins `mergeBuff`. `$keys` in the state dump are sorted by content
     (`state_dump.lua` `keyLabel`, items by id), so the injected order was always 10…23 (Item:20
     first). The test passed only when PoB happened to visit 20 before 23 (`POB_QUERY_CACHE=off`
     ×3: 1 pass, 2 fail). `tests/PF-C.rs` now restores PoB's order from that run's
     `pfc:calcFlaskMods` call order (`restore_flask_order`). Without it, runs 1 and 3 fail on the 4
     mismatches and run 2 passes. With it, all three pass.
   - Not covered: `env.tinctures` (also item-keyed; in `mergeTinctures`' `pairs(tinctures)` loop at
     CalcPerform.lua:1905 the order decides which tinctures fall under `tinctureLimit`) has no call-order probe. No corpus
     build has shown a difference there.
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
   **Closed by FUP2** (port/FUP2, 2026-11-12): `guard_item` and its console line are gone; the
   item DB loaders call `add_unique` / `add_rare` directly, so an item that raises aborts the
   items coroutine as in PoB. Evidence: IT-B `item_core_vs_pob` (all 1,333 uniques and 205 rares
   built, Craft 207/207, 0 failed); pob-ui-oracle `u16_main` (3 tests, `u16_scenarios` included)
   and `u22_notes` (`u22_scenarios`, 8 runs) pass with the guard removed.
4. **Patched-src prune can pull a copy from under a long run** (owner: whoever next touches
   `crates/pob-oracle/src/patcher.rs` `prune`). A `<.parity>/patched-src/<sha>` copy whose
   `.last-used` is older than 24 h (`POB_PATCHED_SRC_MAX_AGE_HOURS`) is renamed away even while
   another process that took it more than 24 h earlier still loads Lua files from it; `keep`
   protects only the pruning process's own sha. Marker-less legacy copies in `target/oracle-src`
   go after 72 h the same way, under an older branch still using them. Unlikely at current run
   lengths; closing it means holding the marker for a run (re-touch it while a process holds the
   copy, or a per-process lock file the prune checks).
   **Closed by FUP3** (port/FUP3, 2026-11-12) for copies this code hands out: `patcher::publish`
   takes a shared `flock` on `<copy>/.in-use.lock` for the rest of the process
   (`patcher::hold_copy`), and `prune` removes a stale copy only while it holds that lock
   exclusively (non-blocking; held across the rename, so a process that takes the copy meanwhile
   finds it gone and republishes). The kernel drops a dead process's lock. Test:
   `patcher::tests::prune_keeps_held_copies`. Still open: copies used by processes of older
   code (no lock; pre-FUP3 branches) keep the 24 h / 72 h age rule.
5. **`runner::shared_patch_root` doc is stale** (owner: the next change to
   `crates/pob-oracle/src/runner.rs` for another reason). It still says "every worktree shares
   one copy per sha" of `target/oracle-src`; since FUP1 `patcher::patch` publishes into
   `patcher::shared_root()` whatever root it is given. Left alone in FUP1 because `runner.rs` is
   `include_str!`'d into `PROBE_RUNTIME`, so any edit retires every cached probe selection.
   **Closed by FUP3** (port/FUP3, 2026-11-12), in the same change as FUP3's other `runner.rs`
   edit (`probe_in_process` installs the encoder's upvalue reader): the doc now says that
   `patcher::patch` publishes into `patcher::shared_root()` and uses the given root only without
   a `.parity`. FUP3 also edits `jit.rs` (`ORACLE_RUNTIME`), so it retires every cached query
   result as well as every probe selection; the patched `<sha>` changes with `state_dump.lua`.
6. **NaN `f64` keys still raise on reads outside item.rs / item_parse.rs** (owner: the units of
   each file; predates FUP1). Helpers such as `val_key` in `calc_active_skill.rs`,
   `init_env_skills.rs`, `item_mods.rs` and `passive_tree.rs` build the read key with
   `Key::from(f64)`, which raises on NaN, where Lua's `t[0/0]` reads nil. FUP1 switched only
   item.rs / item_parse.rs to `try_num`; the others should read through it too.
   **Closed by FUP2** (port/FUP2, 2026-11-12). Each of the four files has a read helper next to
   its store helper (`read_at` in passive_tree.rs, item_mods.rs and calc_active_skill.rs,
   `read_key` in init_env_skills.rs): a nil or NaN key reads nil, any other key as before; stores
   keep `val_key` / `key_of`, so a store at NaN still raises "table index is NaN". Read sites
   switched: PassiveTree.lua 142, 554, 560/565, 589, 612; Item.lua 2261 (`groupCounts`, the read
   half), 2508 (`clusterJewel.skills[…]`); CalcActiveSkill.lua 59 (`stats[stat.key] or 0`, so a nil
   `stat.value` now raises the arithmetic error first, as Lua does); CalcSetup.lua 1532. Also
   fixed there: `explode_key` (CalcSetup.lua 1526/1532, `explodeSource.modSource or
   explodeSource.id`) gave a passive node's typed `id`, NaN for a node with a string id
   (CCR-0024 §1), where Lua keys by that string; it now uses `node_id_val`. Tests
   (`fup2_key_tests` in each file; the explode-key one fails on the old code): 5/5 pass. Neither
   corpus has such a key, so there is no oracle comparison; the Lua semantics are LuaJIT's
   `lj_tab_get` (nil) and `lj_tab_newkey` (`LJ_ERR_NANIDX`).

## PF-B

Final-review open issues, both resolved in the merge commit's follow-up (listed for the record):

1. **CCR number collision (resolved).** PF-B's CCR was `0091-pf-b-minion-fields-and-fn-probes.md`,
   but 0091 is CT's on rust-port (AMENDMENTS A8). Renamed to
   `docs/port/ccr/0095-pf-b-minion-fields-and-fn-probes.md`; every `CCR-0091` marker in
   `crates/pob-calc/src/modules/calc_perform/actor_b.rs` (8) and `crates/pob-oracle/tests/PF-B.rs`
   (2) now reads `CCR-0095`. **Resolved since (CCRDOC check, 2026-11-12):** U16's file is `0096-u16-main-mode-seams.md` and CA's `0097-ca-calcs-seams.md`, so only CT holds 0091. The original note: U16 merged with
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
   **Closed by FUP2**: see "CA" item 2.
2. **Jewel-closure modKeys collide in the port** (owner: CT, `mod_tools.rs` `FUNCTION_TEXT`;
   information). `ModTools.formatValue` prints a jewel `func` as `function: builtin`; PoB prints
   the closure address. The power report keys its cache by `node.modKey` (CalcsTab.lua:613), so
   two nodes whose mods differ only in their jewel closure (same `type`/`radius`) share one cache
   entry in Rust and not in PoB. Not seen in the PWR builds (PWR.rs normalises the address when it
   matches PoB's calls; node 50692 is the one such node there); would show as a power difference
   and a call-count difference in `pwr_l5_replay`.
   **Closed by FUP2** (port/FUP2, 2026-11-12; CCR-0141). `formatValue` prints a jewel closure
   as `function: 0x` and 16 hex digits, an FNV-1a hash of the closure's spec (jewelFuncList key
   or defining closure, and the captures; `mod_tools::function_text`), in place of the fixed
   `function: builtin` (`FUNCTION_TEXT` is now `FUNCTION_TEXT_PREFIX`). Distinct closures get
   distinct texts and modKeys; one closure gets the same text in every process. Readers that
   compare with PoB: PT.rs `fn_text` now applies Lua's `function: 0x%x+` → `table` to the Rust
   text too; PWR.rs `norm_modKey` and u26_power.rs `norm_mod_key` (`function: %w+`) needed no
   change; CT.rs compares no closure (no ModCache entry holds one). Test:
   `mod_tools::fup2_function_text_tests` (fails on the old text).
3. **End-to-end node power is not at parity** (owners: CS2 and the calc units behind CA's L6
   floors). `pwr_l6_report`: upstream_OccVortex Offence/Defence, all nodes, real Rust calculator
   vs PoB 6452/7676 power values exact (Rust defence 0 where PoB has item-dependent defence);
   PWR itself is exact on PoB's calc results (`pwr_l5_replay`, 108,820/108,820). Gate it once
   CA's L6 headline is exact.
   **Closed by FUP2** (port/FUP2, 2026-11-12, `POB_FULL_SUITE=1`): upstream_OccVortex
   Offence/Defence, all nodes, 7,672/7,672 power values exact, 0 missing, 0 Rust-only (the 7,676
   above counted Rust-only values too); parallel vs sequential 7,672/7,672, 0 divergences.
   `pwr_l6_report` now asserts every value exact with none missing or extra, and pins the
   default build's count (`L6_OCCVORTEX_KEYS = 7672`); parallel divergences stay reported only
   (D12).
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
   **Closed by FUP2** (port/FUP2, 2026-11-12). `has_cost` / `cost_of` take the active skill and
   check `skills_tab::levelCostCleared(activeEffect.grantedEffect.id, activeEffect.level)` as
   costs_passes.rs does (CCR-0019 §8, CCR-0133 §2): a cleared level's cost is truthy and has no
   entries. Test: `CA.rs` `ca_level_cost_cleared_reservation`, a helmet's triggered level-20
   Fireball supported by the helmet's High-Impact Mine ("Skills socketed in your Helmet …",
   `appliesToGrantedSkills`): ManaReserved 0, ManaUnreserved 590, ManaCost 0, PoB's values
   (pob_headless, POB_ENGINE=jit). The old code gives ManaReserved 13.

## OF-A

1. **`ofa_l4_setup`: 8 L4 hits differ on ActiveMineLimit / ActiveTrapLimit** (owner: OF-A; found
   at the TP1 merge, 2026-11-12). On upstream_Dual_Savior and upstream_Mirage_Archer_Toxic_Rain the
   offence:362 segment gives 0 in Rust where PoB has 15, with or without the TP1 pool. The end-to-end
   L6 output is exact on both builds, so this is probably an injected-state (harness) gap rather than
   a calc bug, but that is not verified. The full-corpus gate (98 %) passes; the quick corpus has a
   97 % floor for it. Closing it: root-cause it, then restore one 98 % (or 100 %) gate.

## FUP2

Found while running FUP2's acceptance tests (port/FUP2 on rust-port 4c24bc2, 2026-11-12).

1. **The fresh corpus is not exact at L6 or in `ca_l5_post`, on rust-port 4c24bc2 already**
   (owner: CA / INT-1F; information). `CA_CORPUS_DIR=.parity/fresh POB_FULL_SUITE=1` on
   port/FUP2: `ca_l6_corpus` all keys 85,602/85,603 (fresh_076 `ManaCostRaw`: Rust
   45.75809523809523, PoB 45.76), `ca_l5_post` CALCS lists 96/100 clean (fresh_031, _038, _040,
   _089: one `player.breakdown.SkillBuffs` row each). The same three-build subset fails the same
   way on rust-port 4c24bc2 with FUP2's code reverted, and no calc file changed between 840bac2
   (the INT-1F-A pin) and 4c24bc2. Evidence for the two causes:
   - fresh_076: PoB under LuaJIT is not deterministic on this key between processes, and
     nothing here shows a warm process state or a stale cache. Measured on 2026-11-12 with
     `POB_ENGINE=jit pob_headless` and `HEADLESS_EXEC='return string.format("%.17g",
     build.calcsTab.mainOutput.ManaCostRaw)'`, one process per line. The first load in a process
     printed 45.759999999999998 in 7 of 10 processes and 45.75809523809523 in the other 3. The second
     load in the same process printed 45.759999999999998 in all 8 processes that did one. With
     `HEADLESS_RECALC=1` (a second `BuildOutput` after each load), one process printed 45.758...,
     45.76, 45.76 for three loads and another printed 45.76 for both of its loads. The embedded
     interpreter (no `POB_ENGINE`) printed 45.75809523809523 for both loads. A fresh PoB process
     (the CCR-0133 oracle setup, one process per build) can therefore give either value, and both
     are real PoB outputs. The cached oracle value 45.76 is one valid fresh result, and Rust's
     45.758... is the other. The likely cause is D8: LuaJIT seeds the string hash per process, so a
     `pairs()` over a string-keyed table somewhere on the cost path (Will of the Lords, base Mana
     cost 24) iterates in a different order. Rust uses insertion order. That site is not found yet.
     An earlier version of this bullet said that the first load always gave 45.758, the second
     always gave 45.76, and Rust matched the first load. A review then reported 45.76 first and
     45.758 second. Repeated runs showed neither sequence as a fixed pattern, and a second load
     never gave 45.758. Closing it: find the order-dependent
     `pairs` site. Then either show that Rust's insertion order is one of PoB's orders and record
     fresh_076 as a §9.5 nondeterministic key (`scripts/port/oracle.sh nondet` covers
     `.parity/builds` by default, not `.parity/fresh`, and CA.rs's L6 compare reads no nondet
     record), or fix Rust if the order PoB uses is deterministic after all.
   - the 4 `SkillBuffs` rows: the representation gap INT-3 recorded as H3
     (docs/port/parity/INT-3-triage.md §5: PoB dumps an inner mod's empty `tags` as `[]`, the
     Rust export leaves it out); INT-3 fixed it in its own harness's `norm`, CA.rs's L5 compare
     does not have that normalisation yet.

   **Closed by FUP4** (port/FUP4 on rust-port ae0dc52, 2026-11-12).
   - fresh_076 is PoB nondeterminism (§9.5), and Rust's order is one PoB takes. The `pairs` site
     is CalcSetup.lua:1807, `for _, supportGroup in pairs(supportLists[slotName])`: it adds the
     supports of the other Gloves socket groups to the item-granted Will of the Lords. The table
     is keyed by socket-group tables, so PoB visits them in address order. This differs between
     processes. The port already marks the site LUA-ORDER and uses insertion order
     (`calc_setup/init_env_skills.rs` L1807). The order decides the order of the four
     `SupportManaMultiplier` MORE mods (Deadly Ailments 40, Trap and Mine Damage 30, Unbound
     Ailments 20, Swift Assembly 10). ModList.lua `MoreInternal` multiplies them in list order
     and floors the product to 4 digits (Data.lua:551 highPrecisionMods), and CalcOffence.lua:1713 floors it
     again. In the orders 30,40,10,20 and 40,30,10,20 the f64 product is 2.4023999999999996, so
     the floor gives 2.4023 and `ManaCostRaw` is 2.4023 × 24 / 1.26 = 45.758095…. The other
     22 orders give 2.4024 or 2.4024000000000005, which floor to 2.4024, so `ManaCostRaw` is
     45.76. Measured in 14 `POB_ENGINE=jit
     pob_headless` processes, reading the main skill's `effectList` and
     `Tabulate("SupportManaMultiplier")`: 7 used Trap and Mine, Deadly, Swift, Unbound (30,40,10,20),
     which is the socket-group order Rust uses, and gave 45.75809523809523. One used 40,30,10,20
     and also gave 45.758…. Six used 20,30,10,40 or 20,40,30,10 and gave 45.76. Four earlier processes used
     40,30,20,10, 30,20,10,40, 20,30,10,40 and 20,40,30,10, and all four gave 45.76.
     `scripts/port/oracle.sh nondet` takes `--fresh` now (every `.parity/fresh/*.xml`; the records
     go to `.parity/nondet/fresh_NNN.json`). Its run on fresh_076 (`--verify --max-runs 6`, 6
     processes) recorded exactly one output key, `ManaCostRaw: [45.76, 45.75809523809523]`.
     `ca_l6_corpus` now reads the record the way `pob_calc_diff outputs` does. A main-output key
     counts as exact when PoB's value of this run and Rust's value are both recorded PoB values.
     Each such key is listed in the summary. No tolerance changed. The other 99 fresh builds
     have no record, because every one of their values is exact against a single PoB run.
     `.parity` is gitignored, so a checkout without `.parity/nondet/fresh_076.json` fails
     `ca_l6_corpus` on the fresh corpus whenever its PoB run gives 45.76. Run
     `scripts/port/oracle.sh nondet --verify --max-runs 6 .parity/fresh/fresh_076.xml` (about 3
     minutes) or `oracle.sh nondet --fresh` (not run on all 100 builds here) to rebuild it.
   - The 4 `SkillBuffs` rows: INT-3's H3 normalisation is now a shared helper,
     `pob_oracle::mod_json::default_empty_tags`. INT-3's `norm` calls it, and so does CA.rs's
     `canon`, which every L5 row and used-tracking compare goes through.
   - Measured on port/FUP4 (`POB_FULL_SUITE=1`). `CA_CORPUS_DIR=.parity/fresh`: `ca_l6_corpus` 100
     builds, 0 Rust errors, headline 1,300/1,300, all keys 85,603/85,603 (fresh_076 `ManaCostRaw`
     accepted as nondeterministic: PoB's run gave 45.76, Rust gave 45.758…), CALCS lists 300/300,
     used-tracking 900/900, misc 2,200/2,200. `ca_l5_post`: cost warnings, used-tracking and CALCS
     lists each 100/100 clean. `ca_d13_fixed_point`: 100/100 fixed points. all57 (no
     `CA_CORPUS_DIR`, all 5 CA tests): `ca_l6_corpus` headline 741/741, all keys 45,954/45,954 (0
     values accepted as nondeterministic), CALCS lists 171/171, used-tracking 513/513, misc
     1,248/1,248, 0 Rust errors; `ca_l5_post` 56/57/57 clean (1 harness-limited, as before);
     `ca_d13_fixed_point` 62/62. `INT-3` `int3_breakdown_corpus` on fresh_031/_038/_040/_089 with
     the shared helper: 1,102/1,102 entries equal.

## U53

1. **T6 (real SimpleGraphic under Wine) not run.** Wine is still not installed (AMENDMENTS A5), so
   the 15-scene SSIM ≥ 0.995 gate of the U53 row is pending. The e2e scenes to use are
   `crates/pob-ui-oracle/scenarios/e2e/*.pobscript` (T3/T4 green against PoB's Lua with the Rust
   calc). Owner: U53 follow-up once Wine exists.
2. **Tree perf gate (< 4 ms at 1080p): closed by U53 fix round 1.** The memoised cluster
   subgraph connectors (rebuilt every frame before: most of the draw of cluster-jewel builds),
   ahash for the tree's asset/sprite maps, keys without `format!`/interning, a sprite memo per
   icon string, and the hover loop's distance test first (CCR-0145 §9). `tests/u53_perf.rs`
   (asserted in release builds): p50 1.65-2.41 ms, p95 1.86-3.55 ms on the four builds over two
   runs on the loaded machine (were 2.6-3.7 / 2.7-4.5). Run: `CARGO_PROFILE_RELEASE_LTO=false
   CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 scripts/port/cargo.sh test --release -p pob-ui-oracle
   --test u53_perf -- --nocapture`.
3. **Web: tree node icons as grey squares: closed by U53 fix round 1.** Cause: `WebFs::search`
   (`pob-host/src/fs_web.rs`) listed only the IndexedDB tree, not the fetched `src/` VFS, so the
   tree view's `LoadImage` probe of `TreeData/<version>/` found nothing and asked for
   `TreeData/skills-3.jpg` etc.; the sync loads failed, SG used its default texture (grey
   0x7F border) with `ImageSize` (0,0), and the sprites' tc (x / 0) sampled its edge: every
   node icon and mastery was a 127-grey square (pixel-checked). The JPEG decode itself works on
   wasm32 (checked under node). The search now lists the VFS's files too (CCR-0145 §10).
   Headless Chromium (release wasm, button-less `mouseMoved`, wheel and clicks over CDP): icons
   drawn, wheel zoom, hover tooltips with the worker's stat differences, a click allocates.
4. **Unit harness nil rule (CCR-0145 §7), review note of fix round 1.** 16 unit harnesses accept
   a Rust `"nil"` gem/group field as the frozen default (`v == "nil"`, was `lua && v == "nil"`),
   which also hides a Rust nil where PoB writes `false`. `u53_e2e` compares these fields
   verbatim, so the full app is covered; tightening the unit harnesses (compare nil verbatim now
   that the model writes it) needs a re-run of all 16. Owner: U53 follow-up.

## BRAND

1. **Update checker (deferred by user).** User decision 2026-11-12: no update checking for now.
   `launch:CheckForUpdate` / `ApplyUpdate` / the first-run `UpdateCheck` stay stubbed
   (`crates/pob-app/src/launch.rs`, DIVERGENCE(update-stub), no network), and the main-mode
   "Check for Update" button is never shown (`crates/pob-app/src/main_mode/init.rs`,
   DIVERGENCE(update-deferred); UI oracle patch 9 hides it on the Lua side). The port's version
   is the repo-root `VERSION` file (`YYYY.MM.DD.N`, `scripts/port/bump_version.sh`,
   `pob_app::launch::PORT_VERSION`), display only: nothing parses or compares it. Closing this
   needs a user decision on the release feed (where port releases are published, how
   `YYYY.MM.DD.N` is compared), then a port of the update flow against it and the removal of
   the `checkUpdate.shown` divergence and of oracle patch 9's checkUpdate entry. Owner: unassigned
   (user decision pending).

## CCRDOC

Code and doc differences found while deciding CCRs 0043-0145 (AMENDMENTS A14). CCRDOC changed no
code. Each item was checked by reading the code on `rust-port` `f0ef34a`; none was reproduced in
the running app. Items 1-5 and 7 differ from PoB in the product; 6, 8, 9 and 11 are hygiene; 10 is
a sub-pixel draw difference; 12 cannot be reached until a host passes the command line.

1. **Options Save in the build list does not rebuild the list** (CCR-0099 B). Main.lua:1218-1220
   calls `self.modes.LIST:BuildList()` after Save when the mode is LIST (a changed build path
   then shows the new folder). `pob-app/src/main_mode/options_popup.rs:1501-1504` keeps the empty
   `if` from before U18, although `crate::build_list::rebuild(cx)` exists (CCR-0100 B). Fix: call
   it there (one line). Owner: U17 (options_popup.rs). `u17_popups` stubs BuildList on the Lua
   side, so its scenarios do not see the difference.
   **Closed by FUP5** (port/FUP5, 2026-11-13): `options_popup.rs` calls
   `crate::build_list::rebuild(cx)` at L1219. `u17_popups` now runs the real build list as
   `main.modes.LIST` on both sides (the BuildList stub is gone), so `e_options_save`, which sets a
   new build path, sees the list rebuilt: 0 divergences with the fix, 144 without it.
2. **Import "to a new build" draws the old build node for one frame** (CCR-0109 §9).
   `build/on_frame.rs` reads the build node once at the top. After `draw_tab` (ImportTab.lua:
   665-667 re-initialises the build inside the tab's Draw), the rest of OnFrame (L1321-1351)
   draws the old node's controls. Fix: re-read `cx.app().build` after the `draw_tab` call
   (`on_frame.rs:704-706`). Owner: U20. `u43/d_new_build.pobscript` sets frame 30 aside for it.
   **Closed by FUP5** (port/FUP5, 2026-11-13): `on_frame` reads `cx.app().build`'s node again
   after `draw_tab`. The frame-30 set-aside is gone from `d_new_build`: 0 divergences at both
   screens with the fix, 171 without it.
3. **POST requests carry no Content-Type** (CCR-0109 §10). curl's `OPT_POSTFIELDS` sends
   `application/x-www-form-urlencoded`; `pob-host/src/net_native.rs:87` sends the body with
   ureq's `send_bytes` and no default header. This affects UploadBuild and the OAuth token
   requests. Owner: U7.
   **Closed by FUP5** (port/FUP5, 2026-11-13): `pob_host::post_content_type` gives curl's default
   (`application/x-www-form-urlencoded` for a request with a body and no Content-Type header
   line); `net_native.rs` (ureq) and `net_web.rs` (`fetch`, which adds no type for a byte body
   either) both set it. `net_native::tests::download_page_local` checks one form Content-Type on
   a POST without a header line, one `application/json` when a line sets it, and none on a GET.
4. **The comparison build's item and jewel tooltips are built against the open build**
   (CCR-0122 §2, CCR-0143 §4). `ItemsTabHooks::add_item_tooltip` takes no build, so
   `compareSpec.build.itemsTab:AddItemTooltip` (PassiveTreeView.lua:1492/1783) and
   `hoverItemsTab:AddItemTooltip` for an entry item (CompareTab.lua:3988) show the primary
   build's "Equipping this item" blocks where PoB shows the entry's. Owner: U38 with U46-U49.
   **Closed by FUP6** (port/FUP6, 2026-11-13): the hook takes the model build
   (`Fn(&mut Cx, &model::BuildRef, &mut Tooltip, &ItemRef, Option<&Table>, bool)`) and
   `ItemsTabSelf::of_build` selects the entry's ItemsTab for an entry's model; `AddItemTooltip` /
   `AddItemStatDifferences` take `impl Into<ItemsTabSelf>` and run an entry's with the entry
   swapped in (`compare::entry::with_entry_as_build`), over the entry model's slot tables (calc:BM-A
   `slot_IsShown`, now `pub`), with CompareEntry's `AddStatComparesToTooltip` /
   `AddRequirementsToTooltip`. PassiveTreeView.lua:1492/1783 pass `compareSpec.build`, L1501 the
   drawn build, CompareTab.lua:3988 the hovered side's ItemsTab. Scenarios
   `u49/e_entry_items_1280x800` (entry Weapon 1, Helmet, cluster jewel) and
   `u49/f_entry_tree_1280x800` (jewel sockets, overlay and side by side).
5. **The comparison build's config controls have no tooltip** (CCR-0137 §2).
   `CompareHooks::entry_config_var_control` is always `None` (`tabs/compare/init.rs:353`), because
   U41's ConfigTab constructor cannot be built for a second build. Owner: U41 (a constructor for a
   given model) with U47.
   **Closed by FUP6** (port/FUP6, 2026-11-13): `ConfigTab::new_for(build, ForBuild)` builds the tab
   for another build (its constructor and option `tooltipFunc`s run inside `ForBuild::run`, its
   `self.build:AddStatComparesToTooltip` is `ForBuild::add_stat_compares`); CompareEntry's
   LoadFromXML builds one per entry (`entry::new_config_tab`, after the sections load as for the open
   build, DIVERGENCE(init-order)) and `entry_config_var_control` answers from its `var_controls`.
   Left: a copied `tooltipText` drawn without a `tooltipFunc` (only `enemyDamageRollRange`,
   `hideIfInvalid`) is evaluated with the open build, which differs only under `launch.devModeAlt`
   (`tabs/config/ctor.rs` DIVERGENCE(other-build-tooltip-text)). Scenario
   `u49/g_entry_config_1280x800`.
6. **ConfigTab.lua 1302-1334 is covered by two `@pob` markers** (CCR-0121 §1).
   `python3 scripts/port/provenance.py coverage --range Classes/ConfigTab.lua:1-1362` reports FAIL
   with 31 lines covered twice (1302-1315, 1317-1323, 1325-1334): `pob-app/src/tabs/config/ctor.rs:2465`
   (U41) and `pob-calc-core/src/model/config_tab.rs:810` (BM-B). Fix: U41's marker becomes a
   `/// lua:` note (ui doc §10.5, "Provenance of split functions").
   **Closed by FUP7** (port/FUP7, 2026-11-13): `ctor.rs` `update_custom_mods_controls` carries a
   `/// lua:` note naming BM-B's marker. `provenance.py coverage --range
   Classes/ConfigTab.lua:1-1362`: 1231 in-scope lines, 0 uncovered, 0 covered twice. The whole
   scope (`coverage`): 41,266 lines, 0 uncovered, 0 covered twice; `check`: 2357 `@pob`, 208
   `@pob-skip`, 249 `@pob-file`, 0 errors; `pob-oracle --test U0` 6/6 (full and quick corpus).
7. **A dropped build leaks on desktop** (CCR-0142 §3 B). `build/init.rs:759` drops the previous
   `BuildState` without breaking `Env.build`/`MiscCalculator.build` ↔ `calcsTab` and
   `ActiveSkill.socketGroup` ↔ `displaySkillList(Calcs)` (calc doc §3.3 rule 4). Every switch of
   build therefore keeps the old build and its envs alive (LEAK1 measured 100-252 objects per
   build, `leak1_cycles.rs`), and deleting a socket group keeps its last display skills. Only
   the web worker tears down. Fix: a `Build::teardown` (model owner) called before the drop and
   by the Skills tab on group removal.
   **Closed by FUP5** (port/FUP5, 2026-11-13): `Build::teardown` and `SocketGroup::teardown`
   (`pob-calc-core/src/model/mod.rs`) do CCR-0142 §3 A's body. `build/init.rs` calls it on the
   previous `BuildState`'s model before the drop, `worker_web.rs` `run_request` calls it instead of
   its inline copy, and the Skills tab's delete paths (SkillListControl.lua L47, L175, L186) tear
   the removed groups down. Gates: `leak1_cycles.rs` variant (d) (`Build::teardown`, 0 survivors
   on the four builds) and `pob-app/tests/fup5_teardown.rs`: opening a second build frees the
   first (0 of 1156 objects and 0 of 13 socket groups survive; without the fix the build, 200
   objects and all 13 groups did), and the Delete button frees the group and its display skills
   (without the fix both survived). Not covered: a group dropped by the Skills tab's undo/redo,
   a deleted skill set, or a comparison build (CompareTab entries).
8. **Duplicate helper ports** (CCR-0107 §3, 0109 §4, 0112 §2, 0114 §3; ui doc §13 item 15).
   `string:matchOrPattern` has four ports (notable_db.rs, item_db.rs, config/ctor.rs,
   tree_view/mod.rs), LuaJIT `math.random` two (timeless_list.rs, poe_api.rs), and zlib `Inflate`
   two (import/core.rs on miniz_oxide, party.rs on flate2). pob-oracle's LuaJIT `Inflate`/`Deflate`
   natives differ from SimpleGraphic (`""` instead of nil, default level), which the u43/u45
   harnesses work around. Owner: whoever takes the shared helpers (pob-lua for the PRNG).
   **Closed by FUP7** (port/FUP7, 2026-11-13): `pob_lua::string::matchOrPattern` is the one port
   (U41's `ctor::match_or_pattern` and the tabs' `pcall_match` are `PCall` wrappers over it; L0
   `pob-lua/tests/l0_string.rs` `pob_match_or_pattern`: 6025 vectors run against Common.lua
   1038-1079 under LuaJIT, 0 mismatches). `pob_lua::math::{m_random, m_randomseed}` is the one
   PRNG (one per-thread state, as LuaJIT's one per Lua state; L0 `l0_math.rs` `lj_math_random`:
   12,800 draws, 0 mismatches, of which 7,600 on seeds whose `random_seed` rounds differently
   under FMA are checked against an FMA-seeded model, because this host is aarch64 and both the
   oracle's vendored LuaJIT and /usr/bin/luajit (2.1.1788856981) fuse `d*PI+E` in `random_seed`:
   `math.randomseed(-3.3)` then five `math.random(1,1000000)` print `854163 235769 441195 408577
   471319` under both, the FMA model's stream, where the port gives `853552 235769 458773 408577
   440069`. So the port's unfused (x64, CCR-0001) stream for FMA-sensitive seeds is checked only
   against the model (resting on the reasoning that baseline x86-64 has no FMA contraction),
   never against a real x64 LuaJIT; seeds that round the same either way (0, 12.5, 1.7e9, ...) match both LuaJITs
   bit-exactly). SimpleGraphic's
   `Inflate`/`Deflate` are `pob_gamedata::sg_zlib` (the `l_Inflate` loop on miniz_oxide's
   stream API, `nil, zError(err)`; level 9), used by the timeless LUT loader,
   `tabs/import/core.rs`'s adapters (ImportTab, PartyTab, CompareTab) and pob-oracle's LuaJIT
   natives (`jit.rs`), so the u43/u44/u46/u48/u52 rebindings are gone and u45 gained
   `e_partial_code` (a truncated code: invalid on both sides). pob-app no longer depends on
   flate2 or miniz_oxide. Not changed: pob-oracle's pure-Rust interpreter host
   (`src/lua/host.rs`, not the LuaJIT oracle) keeps its own flate2 natives.
9. **Dead or duplicate code left by the seams.** `tabs/items/ctor.rs::flavourLookup` drops every
   entry and is unused (U38's `tooltip::flavour_lookup` is the one used; CCR-0125 §1);
   `on_frame::State.last_*` and `on_frame::StatLineInfo` are fallbacks nothing writes (CCR-0106 §4);
   `compare/init.rs` `State::comparePowerCompareId` is unused (CCR-0138 §3); `compare/power.rs`
   keeps private copies of U49's `ShouldShowRing3`/`AddAbyssSockets` and U47's group helpers
   (CCR-0138 §1); `items/craft_b.rs` copies `checkLineForAllocates` (CCR-0129 §1); `entry.rs`
   repeats three private BM-A helpers (CCR-0137 §1); OF-B's `radius_breakpoints` wrapper could call
   `calcRadiusBreakpointsOpt` directly (CCR-0043). Two code comments in `pob-app/src/build/mod.rs`
   (lines 10, 136) still cite `CCR-0091 A`, U16's pre-renumbering number: they mean CCR-0096 A
   (the CCR files were corrected by CCRDOC). No behaviour change expected.
   **Closed by FUP7** (port/FUP7, 2026-11-13): `ctor::flavourLookup` deleted (its `@pob`
   ItemsTab.lua:62-69 marker is on `tooltip::flavour_lookup`); `on_frame::State.last_*` deleted
   and the `StatLineInfo` downcast fallback dropped (`StatLineInfo` stays as
   `get_sidebar_breakdown`'s argument, built from U21's `stats::StatLine`);
   `init::State::comparePowerCompareId` deleted; `power.rs` calls `CompareTab::ShouldShowRing3` /
   `AddAbyssSockets` (U49) and `GetGemGrantedEffect` / `GetSocketGroupSignature` /
   `GetSocketGroupLabel` (U47); `display_item::checkLineForAllocates` is `pub` and `craft_b.rs`
   calls it; BM-A's `build_xml::{skillsTab_ctor_model, ImportCalcSettings, new_timelessData}` are
   `pub` (the first takes `main.defaultGemQuality`) and `entry.rs` calls them; `skill_types.rs`
   calls `calcRadiusBreakpointsOpt` directly; both comments read `CCR-0096 A`.
10. **The ItemsTab range slider draws its knob from the unrounded value while dragged**
    (CCR-0124 §5). `ctor.rs` `range_slider_overrides` syncs `val` from `modLine.range` only before
    the class Draw, so the drag step's unrounded `5/82` is drawn where PoB draws `round(5/82, 6)`
    (knob x 1163.0 vs 1163.000032). Owner: U33.
    **Closed by FUP5** (port/FUP5, 2026-11-13): a range slider's changeFunc run inside the class
    `Draw` (the drag step) ends with the same `slider.val = modLine.range` copy PoB's next
    `IsShown` makes (`ctor.rs` `sync_after_drag_step`, both the range line slider and the stacked
    ones); a run from `OnKeyDown`/`OnKeyUp` does not, as in PoB. `u35/g_range_lines` now clicks,
    drags and scrolls the slider: 0 divergences with the fix, 4 without it.
11. **Exporter gaps behind CS2's excused buckets** (CCR-0140, test-only). `probe.rs` exports a
    bare `LjIntMap<NodeRef>` (`radiusJewelList[i].nodes`) as `{"$userdata": true}`, and a second
    pass's `override.extraJewelFuncs` actor does not line up path by path. `tests/CS2.rs` excuses
    both within `EXPLAINED_MAX`. Owner: ORC-X (probe.rs).
    **Closed by FUP7** (port/FUP7, 2026-11-13; the second gap in part): probe.rs `PObj::NodeIdMap`
    exports a bare `Rc<LjIntMap<NodeRef>>` as the node map it is, with its own identity (a second
    entry of one socket is a `$ref`, as in PoB). CS2 (all57 + corpus/CS2): the
    `radiusJewelList.nodes` bucket went from 129,887 to 0 and is gone (with `radius_nodes_equal`);
    CS3's `nodesInRadius (opaque export)` (17,835 at L5) is gone too, with CS3 unchanged
    otherwise (L4 336 pass / 1 fail, L5 168 / 3: the `isSupporting` `$keys` order of
    allflame_exp_051, failing the same way in the 2026-11-13 01:14 full suite before FUP7). In
    `tests/CS2.rs` a dumped foreign actor's `itemList` holds the model's items
    (`share_model_items`) and a carry keeps pass 1's player alive in the test instead of
    `override.extra["$extraJewelFuncsActor"]`: the `override.extraJewelFuncs` bucket went from
    17,414 to 1,768, the new `EXPLAINED_MAX` (CS2 still 1104 / 1104 compared hits identical, 6
    restarts, 6 identical second passes). Left (still excused): the tables a dumped foreign actor
    shares with this env's items (`weaponData1/2`, whose PoB canonical path runs through the
    actor while the decoder makes copies) and, on a carry, Rust's pass-1 player against PoB's (its
    dead `enemy`, its own `cachedPlayerDB` parent, eager struct fields). Tightening the excuse to
    carry hits only made 707 of 1104 hits fail, so it stays.

    FUP7 runs: pob-ui-oracle u20_build, u21_stats, u23_tree_view, u27_timeless, u31_skills,
    u32_items, u36_craft_a, u37_craft, u38_tooltips, u41_config, u43_import, u44_character,
    u45_party (with `e_partial_code`), u46_compare, u47_compare, u48_compare_power, u49_compare,
    u52_ext_build_list: all pass; `parity` 54 passed, among them the u28 galleries (notable,
    timeless list and socket: 0 divergences); pob-oracle CS2 (full and quick corpus), U0 (full
    and quick), OF-B, BM-A, INT-3 pass; CS3 as before (see above); pob-lua (27 unit tests, 8 L0
    binaries), pob-gamedata `sg_zlib` and pob-app `--lib` (75) pass. Main.lua:437-443 was
    covered by both `main_mode/on_frame.rs` and `pob-ui-core/src/popups.rs` (outside
    `scope.toml`, so `coverage` did not report it): the popups.rs marker is a `/// lua:` note
    now. Still covered twice outside `scope.toml`: Common.lua:803-828 `formatNumSep`
    (`calc_format.rs` and `luastr.rs`, ui doc §13 item 15's A8 merge) and
    Data/Global.lua:179-184 inside 174-214 in pob-dump's generated `global.rs` (codegen output).
12. **A build link on the command line still raises** (DIVERGENCE(no-cli-args)). Main.lua:71-82
    imports `arg[1]` through `buildSites.ParseImportLinkFromURI`/`DownloadBuild`, which U43 has
    ported (`tabs/import/build_sites.rs`). `pob-app/src/main_mode/init.rs:150-153` still raises
    "needs buildSites ... not ported yet". It cannot be reached in the product, because no host
    sets `launch.arg` (`pob-app/src/launch.rs:93`). Fix: port L72-81 on U43's functions when a
    host passes the command line. Owner: U16 (main_mode/init.rs).
    **Closed by FUP5** (port/FUP5, 2026-11-13): `main_init` ports L71-83 on U43's
    `parse_import_link_from_uri`/`download_build`, and `app/pob-desktop` passes
    `std::env::args().skip(1)` as `launch.arg` (DIVERGENCE(no-cli-args) is gone). `u43_import`
    gained `#! u43-arg LINK` (the Lua side's `arg[1]`, the Rust side's `launch.arg`, network from
    the scenario's `fixture net=`): `e_cli_link` (a pobb.in link: "Imported Build" with its
    `importLink`) and `f_cli_link_error` (a 404: an empty build named after the error), 0
    divergences each. The web host passes no command line.

Dev-only bridge gaps (no product effect, recorded for completeness): `LuaCalcBridge` cannot carry
`override.extra["$removeNodeIds"]` (CCR-0122 §1), the jewel-comparison spec (CCR-0125 §6) or a
NaN-id pseudo node (CCR-0132 §4). It does not use `inject_keep` (CCR-0088 §2) and keeps the
SaveDB side effects (CCR-0114 §6). `pob-ui-oracle` `sg_bridge::deliver_subscripts` frees a
sub-script slot before its callback runs (CCR-0117 §8), and `runner::Driver::frame` drops
`Net::poll` (CCR-0109 §8). Since U53 the unit harnesses run on `RustCalc`, so these gaps matter
only for `POB_UI_CALC=lua` runs.

## TP4

- **u26's Lua UI side is never cached** (owner: the pob-sg trace format, or a TP follow-up in
  `crates/pob-ui-oracle/src/oracle_cache.rs`). With all-zero node power, PoB's heat map calls
  `SetDrawColor` with NaN components (PassiveTreeView.lua, 0/0). `pob_sg::trace::FrameTrace`
  serialises f64 through serde_json, which writes NaN as `null`, and `null` does not read back
  as f64. So `encode_json_checked` refuses the entry (`not stored: its JSON does not read back
  (invalid type: null, expected f64 ...)`), and every u26 run boots its 4 Lua sides (warm 156 s
  against cold 169 s). The fix: give the trace's floats a serde form that carries non-finite
  values (e.g. a `"NaN"`/`"inf"` string for those values only, the same on both sides), or
  store the cache payload in a format that keeps f64 bit-exact. The fix is not in TP4 because
  pob-sg is shared and its trace JSON is also ui-parity's report format.
- **VERIFY fails on entries PoB does not reproduce itself** (owner: TP follow-up,
  `oracle_cache::same_lua_run` / `same_state_doc`). The checks are exact up to `pairs` order of
  lines, attributes, calls and lists. The TP4 VERIFY run (testing.md "Measurements (TP4 fix
  round 1)") found 13 entries in 7 binaries that differ from a fresh run in values PoB itself
  picks per process: the gem variant (SkillsTab.lua:472-478), the flask buff `source`
  (CalcPerform.lua:1773), unique flavour text (ItemsTab.lua:4578-4580), the `masteryEffects`
  value order (PassiveSpec.lua:203) and the deflated export code (u43). Options: recognise these
  classes in the comparison (the variant pick needs `data.gemsByGameId`, as u20's
  `ambiguous_variant` does), or run a fresh Lua side twice on a mismatch and report an entry
  as stale only when the two fresh runs agree with each other and not with the entry. Until
  then, VERIFY failures in u20, u26, u29, u30, u38, u43 and u44 need triage against this list.
