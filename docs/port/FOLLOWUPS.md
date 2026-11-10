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
- **MS-A: the gated Max/Min (EvalMod) queries compare 0 cases** (owner: MS-A). The summary prints
  `Max/Min (EvalMod ported; gated) 0/0`: the probe script produces no Max/Min query on the
  corpus, so that gate is vacuous. Not bounded (nothing to bound); needs probe coverage.

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
- **IT-B `item_core_vs_pob`** (owner: IT-B; the Table limitation is U0's `lua_table.rs`). One
  Craft case fails: `rare#195` (a crafted magic flask with `{range:0.028}`): the port raises
  `non-integral number keys are not supported by Table (929.1)`; PoB crafts the item.

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

## HYG3 exposed

Found while fixing HYG3 (shared oracle caches) on port/HYG3 merged with rust-port ef928ff
(2026-11-10).

1. **OF-C `ofc_l4_costs_passes` fails on allflame_exp_029, allflame_exp_046 and allflame_exp_049**
   (owner: OF-C, the test; the calc is right). `3 of 53 all57 hits differ (> 2 %)`. Each build
   has two `ModExtra` entries in `locals.skillModList.mods`: Rust's state has
   `ChaosMin` / `ChaosMax` BASE 0 from source `Life Flask charges consumed` (the
   PoisonousConcoction `preDamageFunc`, Data/Skills/act_dex.lua:17423-17428) one time more than
   PoB's `offence:2174` dump. Cause: the test still replays PoB's recorded
   `skill:<id>:preDamageFunc` delta onto the state after the segment
   (tests/OF-C.rs, `if let Some(h) = hook`), as its module doc says, because
   `runSkillFunc("preDamageFunc")` (CalcOffence.lua:1908) "is OF-A's still unported local
   function". OF-A has since ported it (`calc_offence/costs_passes.rs:925`
   `runSkillFunc(ctx, SkillHook::preDamageFunc)`), so the segment runs the hook itself and the
   replay applies it a second time. Its `NewMod` calls append, so the mods are doubled. Hooks that
   only assign fields are idempotent, which is why only these three builds differ. Evidence:
   with the replay disabled locally, the test reports `62 builds, 58 hits, 58 clean, 0 with
   mismatches` (and the three builds alone are 3/3 clean). Fix: drop the replay (and the
   `skill:*:preDamageFunc:*@main` part of the selection, with "preDamageFunc replayed in N")
   and update the module doc.
2. **Patched PoB copies accumulate in `target/oracle-src`** (owner: HYG3 / the test tooling).
   Each copy is about 839 MB, one per patch sha (`runner::shared_patch_root`; the main
   checkout's `target/oracle-src` held 5 on 2026-11-10). Any change to the PoB tree,
   `docs/port/stages.toml`, the probe Lua or the Lua runtime versions adds one, and nothing
   removes the old ones. They cannot be pruned automatically while running: another worktree
   on another branch (another `stages.toml`) may be booting PoB from an older copy. For now,
   delete `target/oracle-src/<sha>` dirs by hand when no oracle test is running (they are
   rebuilt on demand). A safe prune would need a use marker (e.g. an mtime touched by
   `patcher::patch` on every reuse) and an age threshold.

## PF-B

Final-review open issues, both resolved in the merge commit's follow-up (listed for the record):

1. **CCR number collision (resolved).** PF-B's CCR was `0091-pf-b-minion-fields-and-fn-probes.md`,
   but 0091 is CT's on rust-port (AMENDMENTS A8). Renamed to
   `docs/port/ccr/0095-pf-b-minion-fields-and-fn-probes.md`; every `CCR-0091` marker in
   `crates/pob-calc/src/modules/calc_perform/actor_b.rs` (8) and `crates/pob-oracle/tests/PF-B.rs`
   (2) now reads `CCR-0095`. **Still open (U16 / CA / CCR3):** U16 merged with
   `docs/port/ccr/0091-u16-main-mode-seams.md` without renumbering, so 0091 is still held twice on
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
