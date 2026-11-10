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

- **u0_smoke: `buildOutput` leaves the player output empty** (owners: CS2, CS3, PF-C..PF-G, CA).
  On all 15 slice5 runs (5 builds x MAIN/CALCS/CALCULATOR) `env.player.output` has 0 keys, the
  enemy output 0 keys and `activeSkillList` 0 entries. Every run hits the stubs
  `CalcSetup.lua:488-799`, `:800-1450`, `:1451-1973`, `CalcPerform.lua:1289-1942`, `:1943-2307`,
  `:2308-2996`, `:2997-3496`, `:3497-3990`, `Calcs.lua:384-870` and `Build.lua:2222-2260`.
  `tests/u0_smoke.rs` now requires a non-empty output for every run that hits none of the
  orchestrator stubs (`ORCHESTRATOR_STUBS`); it no longer asserts that stub sites are hit.
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
