# Splitting `pob-calc` into crates: measured plan (CSPLIT-PLAN)

Status: **plan only**. No code moves in this unit. The implementation is a separate unit that starts
after RETYPE (merged, `4870179`) and INT-1-B merge.
Base of every number below: `rust-port` @ `4870179` (`crates/pob-calc/src`: 94 files, 127,021 lines).
Machine: aarch64, 8 cores, 15 GB shared with about 10 other agents. Toolchain: stable rustc 1.98.1.
Profile: dev (`debug = "line-tables-only"`, pob-calc at `-O0`, incremental on). All builds went through
`scripts/port/cargo.sh` (so `CARGO_BUILD_JOBS=3`, a 5 GB systemd scope and sccache for registry crates).

## 0. Summary

| | monolith today (measured) | 6-crate split (measured on a full-size prototype, §1.2) |
|---|---|---|
| peak RSS of one rustc, cold build | **4.08-4.18 GB** (probe feature: 4.05) | **1.37-1.40 GB** (offence; probe feature: 1.50) |
| peak RSS of all calc rustcs running at once | 4.08-4.18 GB (one process) | **1.80-1.81 GB** (probe feature: 1.99) |
| cold build of the calc, wall | 19.9-20.4 s | **14.5-16.6 s** (probe feature: 15.1) |
| rebuild after a one-statement edit in offence | 3.4-3.7 s, 1.54-1.56 GB | **1.7 s**, 0.64 GB |
| ... in defence | 3.3-3.5 s, 1.52-1.54 GB | **1.9-2.0 s**, 0.59 GB |
| ... in perform | not measured (every measured monolith edit: 3.3-3.7 s) | **0.8-0.9 s**, 0.49 GB |
| ... in core (`ModStore::Sum`) | 3.4 s, 1.53-1.54 GB | 3.8 s, 0.64 GB (all six crates rebuild) |

* The split is worth doing. Two concurrent split builds (2 × ≤ 2.0 GB) need less memory than one
  monolith build does today (4.1 GB). That allows **`POB_PORT_SLOTS=2`** at once, and 3 slots once
  the test-binary compiles are measured as well (§5.3).
* It is feasible. Under the proposed layering only **40 file-to-file edges** point the wrong way.
  They have 8 distinct causes (B1-B8). Besides those, **6 declarations** need `pub(crate)` → `pub`
  (B9), and one partial `calcs` facade is needed (B10). The prototype, made of the real files with only those fixes, compiles with default features, with
  `--features probe` and with `--tests`. Its in-crate unit tests were not run (§5.4, §8).
* Five calls must be bound late, from a lower crate to a higher one: `configApply`, `BuildOutput`,
  `MiscCalculator::call`, `doActorLifeMana` and `doActorLifeManaReservation`. None of them is on a
  hot path. The mechanism (§3.4) is a link-time symbol seam with no init call and no runtime cost.
  It was verified on lld, GNU ld, release thin-LTO, wasm32 and core-only unit tests. It needs a local
  `allow(unsafe_code)` in two small macro-generated modules. **That is an orchestrator decision
  (CCR).**
* The cheapest alternative on its own is `debug = 0` for pob-calc: 3.0 GB peak, 17 s (§6). It does
  not get near the split's numbers, but it stacks with the split.

## 1. Method

### 1.1 Dependency graph: name resolution, not `use` lines

Every module does `use crate::prelude::*`, and the prelude re-exports items from 11 modules. A grep
for `use` lines therefore shows almost everything depending on almost everything. Instead, a
resolver (`depgraph.py`, about 500 lines of Python, written for this unit) does the following:

1. It lexes every file and drops comments, strings and char literals. It tracks `mod` / `impl` /
   `fn` nesting and `#[cfg(test)]` modules.
2. It builds the module tree (file modules and inline `mod x { }`) and each module's items, together
   with their kind and visibility.
3. It parses every `use` tree, including explicit, aliased (`calc_tools as calcLib`), glob, `super::`
   and fn-local `use`.
4. It resolves **every identifier token and every path**. Resolution goes through local items, then
   explicit imports, then glob imports (so a prelude name counts only where the file actually uses
   it), then `pub use` re-export chains (e.g. `crate::calcs::buildActiveSkill` → `modules/calcs.rs`),
   and finally `#[macro_export]` macros. The result is the file that **defines** the item.
5. It resolves method calls `.m(` to the file that defines method `m` in an `impl`, if exactly one
   file does. It excludes common std names, for example `with` (thread-local `LocalKey::with`). 22
   names are ambiguous and left unresolved (`table`, `ipairs`, `wipe`, `to_val`, `is_nil`,
   `copyTable`, ...). All of them are `lua_table`/core helpers, so they cannot add an edge across
   layers.
6. It records `impl` blocks whose self type is defined in another file: **215**, of which 202 are
   inherent impls and 13 trait impls. An inherent impl must live in the same crate as its type.

Result: 683 file→file edges by name and 236 by method, plus 19 edges that only `#[cfg(test)]` code
uses. 3 `crate::stub::*` paths stay unresolved. They are re-exports of `pob_gamedata::stub`, so they
are external.

Limits: the resolver does not expand macros (macro-generated items can be missed) and cannot type
method receivers. §1.2 closes both gaps with the compiler.

### 1.2 Compile prototype: the compiler checks the layering

`proto.py` copies the real sources into six throwaway crates outside the repo
(`core, setup, config, defence, offence, perform`, 127,051 lines in total). Files are copied
**whole and unchanged**, except for the B1-B10 fixes in §3.3, each of which is one to a few lines.
Each crate gets a "mirror root" `lib.rs` (§3.5), so that moved files keep their `crate::…` paths.
`visfix.py` then runs `cargo check` repeatedly. On every privacy error (E0603/E0616/E0624) it widens
exactly that declaration and records it. Results:

* `cargo check` of all six crates passes: default features, `--features probe`, and `--tests` with
  probe. After the B-fixes the only extra changes needed were **6** visibility widenings.
* The resolver's predictions held. The first compile of the core crate passed with no further
  edits. The probe-feature errors were exactly the predicted lines (probe.rs:3210 `run_segment`, and
  probe.rs:1014-1022 `FlaskUpvalues`).
* The late-bound calls are stubs in the prototype (`unimplemented!()`), so the prototype measures
  compile cost, not runtime. A separate experiment verifies the seam itself (§3.4).

### 1.3 Compile measurements

GNU `time` is not installed on this machine. Instead, `ptime.py` takes `ru_maxrss` from
`wait4()`, which is the number `/usr/bin/time -v` prints as "Maximum resident set size". It is
applied two ways:

* around `scripts/port/cargo.sh build -p pob-calc --lib`, which is what the scope asked for;
* around **each rustc**, through a `RUSTC_WRAPPER` that runs `pob_*` crates directly and passes every
  other crate on to sccache. With sccache as the wrapper, rustc runs inside the sccache server and
  its memory is invisible to `time`.

A sampler (`rsssum.py`, every 0.2 s) adds up the RSS of all rustc processes of one build that run at
the same time. "Cold" means the pob-calc incremental directory was deleted and `lib.rs` touched; the
dependencies were already built. The machine is shared, so expect ±5-10 % noise; repeated runs are
listed. When dependencies compiled at the same time, pob-calc got fewer jobserver tokens and peaked
lower (2.93 GB in 27.9 s). Such runs are marked and are not used as the baseline.

The scripts live in the scratchpad of the CSPLIT-PLAN session and are not committed (scope: this
document is the only file this unit adds). The implementation unit should commit `depgraph.py` as
`scripts/port/calc_depgraph.py` so the graph can be measured again.

## 2. Module dependency graph (measured)

### 2.1 Modules, lines, uses

"uses" counts resolved references (names and methods) from the row's files into another module.
Directories under `modules/` are one row each.

| module | files | lines | crate (P1) | uses (refs) |
|---|---:|---:|---|---|
| `active_skill` | 1 | 216 | core | cfg 19, lua_table 19, actor 6, classes/mod 6, model/mod 6 |
| `actor` | 1 | 187 | core | lua_table 16, classes/mod 8, active_skill 2, breakdown 1 |
| `breakdown` | 1 | 48 | core | lua_table 8, actor 4, classes/mod 2 |
| `cfg` | 1 | 122 | core | lua_table 6, classes/mod 1 |
| `classes/eval_mod` | 1 | 1886 | core | classes/mod_store 33, lua_table 15, classes/mod 14, actor 11, cfg 4, modules/calc_tools 4, classes/item 1 |
| `classes/item` | 1 | 2508 | core | lua_table 324, classes/mod 20, classes/item_parse 2, classes/item_tools 1 |
| `classes/item_mods` | 1 | 1530 | core | lua_table 167, classes/mod 16, classes/item 9, classes/mod_store 9, classes/mod_db 7, classes/item_tools 3, classes/item_parse 1 |
| `classes/item_parse` | 1 | 2980 | core | lua_table 260, classes/item 60, classes/mod 6, classes/item_tools 1, classes/item_mods 1 |
| `classes/item_tools` | 1 | 979 | core | lua_table 45, classes/item_parse 1 |
| `classes/mod` | 1 | 327 | core | lua_table 69, actor 1, model/mod 1 |
| `classes/mod_db` | 1 | 742 | core | classes/mod 44, classes/mod_store 41, lua_table 14, cfg 6, classes/eval_mod 6, classes/mod_list 3 |
| `classes/mod_list` | 1 | 624 | core | classes/mod 40, classes/mod_store 24, lua_table 7, cfg 6, classes/eval_mod 6, classes/mod_db 3 |
| `classes/mod_store` | 1 | 1603 | core | classes/mod 55, lua_table 43, cfg 21, classes/mod_db 16, classes/mod_list 8, classes/eval_mod 2, active_skill 1 |
| `classes/passive_spec` | 1 | 2685 | core | lua_table 148, classes/mod 84, model/mod 19, classes/passive_tree 16, classes/passive_spec_paths 5, classes/passive_spec_cluster 3, classes/mod_db 1 |
| `classes/passive_spec_cluster` | 1 | 2253 | core | lua_table 194, classes/mod 62, classes/passive_tree 17, classes/passive_spec 5, classes/mod_db 5, model/mod 1, classes/mod_store 1, classes/passive_spec_paths 1 |
| `classes/passive_spec_paths` | 1 | 1857 | core | lua_table 176, classes/mod 48, classes/passive_spec 9, classes/passive_tree 9 |
| `classes/passive_tree` | 1 | 1812 | core | lua_table 226, classes/mod 40, classes/mod_db 1, classes/mod_store 1 |
| `env` | 1 | 264 | core | lua_table 49, classes/mod 40, actor 8, model/mod 2, active_skill 1 |
| `flow` | 1 | 26 | core | |
| `global_cache` | 1 | 67 | core | lua_table 11, env 5, active_skill 1 |
| `lua_table` | 1 | 1502 | core | |
| `model/build_xml` | 1 | 1027 | core | lua_table 66, model/mod 48, model/xml 4, model/config_tab 3, model/skills_tab 3, classes/passive_spec 1, classes/passive_tree 1, modules/common 1, **modules/power 1** |
| `model/calcs_tab` | 1 | 310 | core | lua_table 46, model/mod 14, model/skills_tab 2 |
| `model/config_build_mod_list` | 1 | 232 | core | lua_table 38, model/config_tab 4, classes/mod 2, model/mod 2, classes/mod_db 2, **modules/config_options/ 1** |
| `model/config_tab` | 1 | 1008 | core | lua_table 138, model/mod 38, model/skills_tab 9 |
| `model/items_tab` | 1 | 1553 | core | lua_table 135, model/mod 53, classes/mod 11, classes/item 4, classes/passive_spec_cluster 2, model/build_xml 1, classes/item_parse 1, classes/mod_store 1 |
| `model/mod` | 1 | 264 | core | lua_table 33, classes/mod 18, active_skill 6, actor 4, env 3, **modules/calcs 1** |
| `model/party_tab` | 1 | 1298 | core | lua_table 100, model/mod 21, classes/mod 6, model/skills_tab 5, classes/mod_db 2, classes/mod_store 2 |
| `model/skills_tab` | 1 | 1358 | core | lua_table 80, model/mod 51, modules/calc_tools 4 |
| `model/tree_tab` | 1 | 238 | core | model/mod 20, lua_table 11, classes/passive_spec 4, model/build_xml 3, model/items_tab 3, classes/passive_tree 1 |
| `model/xml` | 1 | 384 | core | model/mod 24, lua_table 12 |
| `module_state` | 1 | 30 | core | breakdown 1, lua_table 1 |
| `modules/calc_base` | 1 | 8 | core | |
| `modules/calc_breakdown` | 1 | 677 | core | lua_table 71, breakdown 10, classes/mod_store 7, cfg 3, actor 2, modules/calc_tools 2, classes/mod 1 |
| `modules/calc_format` | 1 | 376 | core | lua_table 54, cfg 4, classes/mod_store 3, active_skill 2, actor 1, classes/mod 1 |
| `modules/calc_tools` | 1 | 780 | core | lua_table 14, classes/mod_store 5, active_skill 4, cfg 3, classes/mod 3, model/mod 1 |
| `modules/common` | 1 | 283 | core | lua_table 33, classes/mod 8, global_cache 5, env 4, classes/mod_db 4, active_skill 3 |
| `modules/jewel_funcs` | 1 | 1186 | core | lua_table 36, classes/mod 29, classes/mod_store 6, classes/mod_db 4, classes/mod_list 1 |
| `modules/mod` | 1 | 21 | core | |
| `modules/mod_tools_env` | 1 | 57 | core | lua_table 15, classes/mod 3, classes/mod_db 2, env 1, classes/mod_store 1 |
| `prelude` | 1 | 35 | core | classes/mod 15, model/mod 12, lua_table 9, env 7, actor 5, active_skill 4, cfg 3, breakdown 2, global_cache 2, flow 1, stub 1 |
| `probe` | 1 | 3371 | core | lua_table 323, env 60, classes/mod 55, model/mod 30, active_skill 27, actor 23, cfg 18, **modules/calc_offence/ 14, modules/calc_perform/ 12, modules/calc_defence/ 8, modules/calc_setup/ 4, data_fns/ 2**, global_cache 6, ... |
| `stub` | 1 | 16 | core | |
| `modules/calc_active_skill` | 1 | 2504 | setup | lua_table 169, classes/mod_store 25, active_skill 19, env 14, classes/mod 12, modules/calc_tools 12, classes/mod_db 7, actor 4, modules/common 4, model/mod 3, cfg 2, modules/calc_setup/ 1 |
| `modules/calc_setup/` | 7 | 5049 | setup | lua_table 446, classes/mod_store 204, classes/mod 78, env 58, model/mod 30, active_skill 16, classes/mod_db 15, flow 12, actor 9, modules/calc_tools 8, cfg 7, modules/calc_active_skill 5, classes/item 3, model/items_tab 3, model/skills_tab 3, classes/mod_list 3, modules/common 2, probe 2, classes/passive_tree 2, modules/mod_tools_env 1, modules/jewel_funcs 1 |
| `modules/config_options/` | 3 | 17021 | config | classes/mod 1068, lua_table 725, classes/mod_store 671, model/mod 538, **data_fns/ 536**, classes/mod_db 30, model/config_tab 9, modules/mod_map 3, stub 2 |
| `modules/mod_map` | 1 | 1448 | config | classes/mod 88, classes/mod_store 55, lua_table 51, **data_fns/ 42** |
| `modules/calc_defence/` | 8 | 11219 | defence | lua_table 1434, classes/mod_store 437, modules/calc_tools 50, actor 31, modules/calc_breakdown 21, flow 18, classes/mod 13, env 10, breakdown 7, **modules/calc_perform/ 4**, cfg 4, probe 2, classes/mod_db 2, active_skill 1 |
| `data_fns/` | 5 | 7271 | offence | lua_table 794, classes/mod_store 202, active_skill 148, breakdown 136, modules/calc_tools 7, actor 5, env 4, classes/mod 4, cfg 2, modules/calc_offence/ 2, model/mod 2, global_cache 2, stub 1, classes/mod_db 1 |
| `modules/calc_offence/` | 10 | 20420 | offence | lua_table 2509, classes/mod_store 751, modules/calc_tools 97, cfg 80, modules/calc_breakdown 53, flow 34, breakdown 33, env 19, data_fns/ 18, active_skill 16, classes/mod 15, actor 13, modules/calc_defence/ 10, module_state 8, probe 4, **modules/calc_perform/ 3**, modules/common 3, classes/mod_db 3, modules/calc_format 2 |
| `modules/calc_mirages` | 1 | 1382 | perform | lua_table 70, classes/mod_store 29, env 28, active_skill 24, global_cache 7, modules/common 5, actor 3, modules/calc_triggers/ 2, breakdown 1, classes/mod 1, modules/calc_active_skill 1, modules/calc_offence/ 1, modules/calc_perform/ 1, modules/calc_tools 1, modules/calcs 1, classes/mod_db 1 |
| `modules/calc_perform/` | 8 | 13355 | perform | lua_table 1025, classes/mod_store 877, classes/mod 128, classes/mod_db 58, actor 38, modules/calc_tools 27, env 23, cfg 16, active_skill 15, flow 15, modules/calc_defence/ 9, probe 7, modules/common 6, modules/calc_offence/ 4, breakdown 4, modules/calc_breakdown 3, global_cache 2, modules/calc_triggers/ 2, modules/calc_active_skill 2, modules/mod_tools_env 2, model/mod 2, modules/calcs 1, modules/calc_setup/ 1, modules/calc_mirages 1, model/party_tab 1 |
| `modules/calc_triggers/` | 3 | 5065 | perform | lua_table 361, classes/mod_store 48, active_skill 44, env 25, modules/common 13, global_cache 10, modules/calc_tools 9, actor 8, modules/calc_setup/ 6, modules/calcs 4 |
| `modules/calcs` | 1 | 1690 | perform | lua_table 166, env 33, classes/mod_store 19, model/mod 6, modules/calc_setup/ 6, modules/calc_perform/ 5, actor 4, classes/eval_mod 4, active_skill 3, modules/common 3, classes/mod 2, global_cache 2, breakdown 1, cfg 1 |
| `modules/power` | 1 | 1752 | perform | lua_table 187, model/mod 38, classes/mod 28, env 11, modules/calcs 8, model/build_xml 7, classes/passive_tree 2, classes/mod_store 2 |
| `calcs_facade` | 1 | 49 | perform | modules/calc_defence/ 10, modules/calcs 7, modules/calc_active_skill 5, modules/calc_offence/ 4, modules/calc_perform/ 4, modules/calc_setup/ 4, modules/common 3, env 2, modules/calc_triggers/ 2, model/mod 1 |
| `lib` | 1 | 66 | facade | lua_table 5, env 2, model/mod 2 |

**Bold** marks references that point upward under the layering of §3 (the B-items).

### 2.2 Cycles

Strongly connected components (Tarjan):

* **Module level** (the 57 rows above, with each `modules/<dir>/` as one node): **one SCC of 44
  modules and 106,049 lines**, plus 13 singletons that are pure sources or sinks: `config_options/`,
  `mod_map`, `flow`, `lua_table`, `stub`, `prelude`, `calc_base`, `modules/mod`, `model/calcs_tab`,
  `model/config_build_mod_list`, `model/tree_tab`, `lib` and `calcs_facade`.
* **File level**: one SCC of **79 files / 106,049 lines** and one of the 3 `config_options` files
  (17,021 lines; `mod.rs` ↔ `apply_a`/`apply_b` through `ROWS` and `ConfigApply`). The other 12
  files are acyclic.

All 2-cycles (mutual uses) at module level, with refs each way:

| A | B | A → B | B → A |
|---|---|---|---|
| `active_skill` | `actor` | 6: ActorRef, ActorWeak | 2: ActiveSkillRef |
| `active_skill` | `model/mod` | 6: GemInstance, GemInstanceRef, SocketGroupRef | 6: ActiveSkillRef, SkillEffectRef |
| `actor` | `breakdown` | 1: Breakdown | 4: ActorRef, ActorWeak |
| `actor` | `classes/mod` | 8: ItemRef, ModDB, ModStore | 1: ActorWeak |
| `classes/eval_mod` | `classes/mod_store` | 33: arith, GetStat, GetMultiplier | 2: EvalMod |
| `classes/item` | `classes/item_parse` | 2: ParseRaw | 60: specToNumber, normaliseModLine, is_lineFlag |
| `classes/item_mods` | `classes/item_parse` | 1: rareLikeUnique_ref | 1: CanHaveMod |
| `classes/item_parse` | `classes/item_tools` | 1: applyRange | 1: main_defaultItemAffixQuality |
| `classes/mod` | `model/mod` | 1: Build | 18: ModList, ModStore, ItemRef |
| `classes/mod_db` | `classes/mod_list` | 3: AddMod_List, AddList_List, Print_List | 3: same_params, AddMod |
| `classes/mod_db` | `classes/mod_store` | 41: flags_match, source_match, or0 | 16: AddMod, ReplaceModInternal_DB, ConvertModInternal_DB |
| `classes/mod_list` | `classes/mod_store` | 24: arith, highPrecision, or0 | 8: ReplaceModInternal_List, ConvertModInternal_List, SumInternal_List |
| `classes/passive_spec` | `classes/passive_spec_cluster` | 3: BuildClusterJewelGraphs | 5: spec_stubs, GetSocketedJewel, ImportFromNodeList_impl |
| `classes/passive_spec` | `classes/passive_spec_paths` | 5: BuildAllDependsAndPaths | 9: NodesInIntuitiveLeapLikeRadius, spec_stubs, ... |
| `env` | `model/mod` | 2: BuildRef | 3: EnvRef, ExplodeSource |
| `data_fns` | `modules/calc_offence` | 2: calcSkillDuration | 18: SkillHook, SkillFuncImpl, skillFunc |
| `model/build_xml` | `modules/power` | **1: BuildOutput** | 7: build_extra, loadBuildFromXML, SaveDB |
| `model/mod` | `modules/calcs` | **1: MiscCalculator** | 6: BuildRef |
| `modules/calc_active_skill` | `modules/calc_setup` | 1: initEnv | 5: createActiveSkill, buildActiveSkillModList |
| `modules/calc_defence` | `modules/calc_perform` | **4: actionSpeedMod, doActorLifeMana, doActorLifeManaReservation** | 9: defenceForConditionals, defence, buildDefenceEstimations |
| `modules/calc_defence` | `probe` | 2: ctx_struct | **8: seg, DefenceCtx, DefEstCtx** |
| `modules/calc_mirages` | `modules/calc_perform` | 1: perform | 1: mirages |
| `modules/calc_offence` | `modules/calc_perform` | **3: actionSpeedMod** | 4: offence, calcTotemLife, calcSkillDuration |
| `modules/calc_offence` | `probe` | 4: ctx_struct, skill_hook_handled | **14: seg_ailmentPrelude, seg_bleedPoison, ...** |
| `modules/calc_perform` | `modules/calcs` | 1: buildActiveSkill | 5: perform |
| `modules/calc_perform` | `probe` | 7: ctx_struct, unmark_absent, rc_ptr | **12: seg, FlaskUpvalues, TinctureUpvalues** |
| `modules/calc_setup` | `probe` | 2: unmark_absent, ctx_struct | **4: seg, InitEnvCtx** |

Most 2-cycles stay inside one proposed crate: classes/model/actor/env in core, `data_fns` ↔
`calc_offence` in offence, active_skill ↔ setup in setup, and perform ↔ mirages ↔ calcs in perform.
The bold directions are the cycle-forming edges across crates. Every cycle in the big SCC runs
through at least one of them. Removing them (B1-B10, §3.3) leaves the crate graph a DAG, and the
compile prototype confirms this.

### 2.3 Constraints the graph does not show

* **Inherent impls** (`impl T {}` in another file than `T`): 202. All are inside `classes/`
  (`Item`, `ModStore`, `PassiveSpec`, `PassiveTree` on `classes/mod.rs` types) and `model/`
  (`Build` and the tabs on `model/mod.rs` types), plus `calc_breakdown.rs` → `Breakdown`. All of
  them stay in core, so none crosses a crate.
* **Trait impls on other files' types** (13): `probe.rs` impls its own `ProbeField`/`Slot` for core
  types (8), `costs_passes.rs` impls its own `BreakdownVal` for `Breakdown`, `calc_tools.rs` impls
  its own `GemLevelTarget` (2), and there are 2 `Drop` impls inside core (`PassiveSpec`,
  `ItemsTab`). The orphan rule is satisfied (local trait) in every layout.
* **Restricted visibility**: 833 `pub(crate)`/`pub(super)` markers, most of them in `apply_a/b`
  (532), `data_fns` (133) and `mod_map` (41). Only **6** are used across the new boundaries (found
  by the compiler, §1.2).
* **`cfg(feature = "probe")` inside exported macros**: `ctx_struct!` and `probe!` contain
  `#[cfg(feature = "probe")]`, which is evaluated in the *invoking* crate. Every sub-crate therefore
  needs its own `probe` feature that forwards to core's. Otherwise `ProbeState` impls lose their
  methods. The prototype does this.

## 3. Proposed layering

### 3.1 Crates

| crate | contents (paths unchanged under `src/`) | lines | depends on | measured cold peak / rustc wall |
|---|---|---:|---|---|
| `pob-calc-core` | `lua_table cfg flow stub breakdown global_cache module_state actor active_skill env prelude probe` (minus `run_segment`), `classes/*`, `model/*`, `modules/{calc_tools,common,calc_format,calc_breakdown,mod_tools_env,jewel_funcs,calc_base}`; new: `late.rs`, `data_fns_types.rs`, `modules/calc_perform_shared.rs` | 38,741 | pob-lua, pob-gamedata, pob-modparser | 0.73 GB (probe 0.78) / 8.6-10.1 s |
| `pob-calc-setup` | `modules/calc_setup/*`, `modules/calc_active_skill.rs` | 7,581 | core | 0.37 GB / 2.8-4.2 s |
| `pob-calc-config` | `modules/config_options/*`, `modules/mod_map.rs` | 18,500 | core | 0.39 GB / 2.3-2.6 s |
| `pob-calc-defence` | `modules/calc_defence/*` | 11,247 | core | 0.56-0.57 GB / 3.7-5.0 s |
| `pob-calc-offence` | `modules/calc_offence/*`, `data_fns/*` | 27,683 | core, defence | 1.37-1.40 GB (probe 1.50) / 8.3-9.8 s |
| `pob-calc-perform` | `modules/calc_perform/*`, `modules/calc_triggers/*`, `calc_mirages.rs`, `calcs.rs`, `power.rs`, `calcs_facade.rs`; new: `probe_segments.rs` (`run_segment`), `late_provider.rs` | 23,299 | core, setup, config, defence, offence (config only for the seam provider, §3.4) | 0.88-0.97 GB (probe 0.86) / 5.1-5.4 s |
| `pob-calc` (facade) | `lib.rs`: re-exports only (§4) | ~60 | all six | (not built in the prototype; a re-export crate) |

Line counts come from the prototype. They include the ~30 lines per crate of mirror roots and the
moved items.

Dependencies (each arrow reads "is used by"):

```
pob-lua → pob-gamedata → pob-modparser → pob-calc-core
pob-calc-core    → pob-calc-setup, pob-calc-config, pob-calc-defence      (compile in parallel)
pob-calc-defence → pob-calc-offence
core + setup + config + defence + offence → pob-calc-perform
all six → pob-calc (facade) → pob-oracle, pob-app, pob-ui-core, pob-ui-oracle
late-bound at link time (core calls, perform provides): configApply, BuildOutput,
    MiscCalculator::call, doActorLifeMana, doActorLifeManaReservation
```

Why this cut:

* **core** holds everything the inherent impls tie together (`classes/` and `model/` and their
  types), plus the libraries every calc module uses (`calc_tools`, `common`, `calc_breakdown`,
  `calc_format`, `jewel_funcs`, `mod_tools_env`).
* The **defence / offence / perform** order follows the measured edges. perform → offence (4 refs)
  and offence → defence (10) point down. Only `actionSpeedMod` and two `doActorLifeMana*` calls
  pointed up (B2, B3).
* **setup** and **config** depend on core only, so they compile in parallel with defence.
* **data_fns** sits with offence, the only crate that calls into it (`skillFunc`/`SkillHook`). Its
  reverse edge `calcSkillDuration` (2 refs) is then crate-internal.
* Alternatives considered:
  * P2: defence merged into perform, with `calc_defence/helpers.rs` moved to core. It needs no
    `doActorLifeMana*` seam, but perform grows to 32.6k lines and defence edits rebuild a bigger
    crate.
  * Keeping `config_options` in core: one seam fewer, but core grows to 57k lines.
  * Both are worse on the measured cost of peak memory and of edits.

### 3.2 Hot paths

* **Dev (-O0)**: nothing is inlined in either layout, so cross-crate calls cost the same as
  intra-crate calls.
* **Release** (`lto = "thin"`, `codegen-units = 1`):
  * The monolith is one LLVM module. After the split, cross-crate inlining of hot core accessors
    (`Val::num`, `Table::get`/`set`, `ModStore::Sum`/`Flag`) relies on thin LTO, which does
    cross-module importing. Only 10 functions in `lua_table.rs` and 5 in `mod_store.rs` carry
    `#[inline]` today.
  * Expect parity, within noise, but measure it: INT-2 owns the power-report timing gate
    (calc doc §11, "power report ≤ LuaJIT time"). Run `pob_timing` before and after, and add
    `#[inline]` to tiny cross-crate accessors if the numbers show a difference.
* **The 5 late-bound calls** are cold paths:
  * `configApply`: once per config var per `BuildModList`;
  * `BuildOutput`: once per build load;
  * `MiscCalculator::call`: per power/what-if calc, which is a full `initEnv` + `perform` that
    dwarfs one indirect call;
  * `doActorLifeMana[Reservation]`: once per actor per defence pass.

  With the link-time seam they are direct calls anyway (§3.4).
* Generic functions are instantiated in the crate that uses them. Dev builds share them
  (`share-generics` is on by default at opt-level 0). Release may duplicate a few instantiations,
  which costs code size, not speed.

### 3.3 Breaking each cycle across crates (B1-B10)

Evidence lines are at `4870179`. The prototype applied each fix as listed.

| # | edge (file:line) | fix | cost |
|---|---|---|---|
| B1 | `config_options/apply_a.rs`, `apply_b.rs`, `mod_map.rs` → `data_fns/mod.rs` `FnRow` (578 refs, via `config_options/mod.rs:13` `pub use crate::data_fns::FnRow`); `probe.rs:3112,3125` → `SkillHook` | **move types down**: `SkillHook`, `SkillFunc`, `ExplosiveArrowFunc`, `SkillFuncImpl`, `FnRow` (`data_fns/mod.rs:18-63`) → new core file `data_fns_types.rs`. `data_fns/mod.rs` keeps `pub use crate::data_fns_types::{…}`, so `pob_calc::data_fns::FnRow` etc. are unchanged. `config_options/mod.rs:13` and `probe.rs` import from `crate::data_fns_types` | 0 runtime; ~45 lines moved, 3 imports edited |
| B2 | `actionSpeedMod` (`calc_perform/actor_b.rs:2276-2303`), called from `calc_defence/est_ehp.rs:20/966`, `primary.rs:24/82`, `calc_offence/ailments_b.rs:30/2018`, `helpers_setup.rs:30/800`, `crit.rs:920` (`calcs::actionSpeedMod`) | **move the function down** (28 lines with comments; it only uses `modDB`/`data()`): `@pob CalcPerform.lua:1141-1151` marker and body → new core file `modules/calc_perform_shared.rs`. `actor_b.rs` keeps `pub use …::actionSpeedMod` (the `calc_perform::actionSpeedMod` and `calcs.actionSpeedMod` paths stay). Change 4 `use` lines | 0 runtime. The function now lives outside `calc_perform/`, an exception to D1's file↔Lua mirroring that the PF-B owner signs off in the CSPLIT CCR. Its `@pob` marker moves with it (provenance.py scans all of `crates/`) |
| B3 | `calc_defence/primary.rs:100` `doActorLifeMana`, `:109` `doActorLifeManaReservation` (`calc_perform/actor_a.rs:266`, `:1514`) | **late seam** (§3.4), inside the existing `call_doActorLifeMana*` wrappers (primary.rs:94-110); the replay hooks there are unchanged | 2 lines + 2 seam entries |
| B4 | `model/mod.rs:21,218` → `modules/calcs.rs` `MiscCalculator` (`CalcsTab.miscCalculator` field) | **move the struct down** (2 fields, core types) into `late.rs`. It keeps an **inherent** `pub fn call(&self, …) -> Output` that calls the seam. The body stays in `calcs.rs` as `fn MiscCalculator_call(this, …)` with its marker. It must stay inherent: `pob-app/src/calc/rust.rs:51` calls `self.0.call(…)`, and `pob-oracle/tests/CA.rs:369,1416` sets and gets the typed field. `calcs.rs` keeps `pub use …::MiscCalculator` | 1 seam entry; ~10 lines |
| B5 | `model/build_xml.rs:450` → `modules::power::BuildOutput(build)` | **late seam** | 1 line + 1 entry |
| B6 | `model/config_build_mod_list.rs:13,112` → `modules::config_options::configApply` | **late seam**. The `ConfigApply` fn-pointer alias (`config_options/mod.rs:16`) moves to core, and `config_options` re-exports it. `ConfigTab::BuildModList` stays inherent in core, because pob-app calls it as a method at 18 sites | 1 line + 1 entry |
| B7 | `probe.rs:3202-3371` `run_segment` (the stage dispatcher, imports `calc_setup/perform/defence/offence` at :3210) | **move the function up**: probe.rs:3202-3371 → new file `probe_segments.rs` in perform. The facade's `probe` module re-exports it (§4), so `pob_calc::probe::run_segment` (23 files in pob-oracle) is unchanged | 0 runtime; feature `probe` only |
| B8 | `probe.rs:137,1014-1022` reflect `FlaskUpvalues`/`TinctureUpvalues` (`calc_perform/setup_flasks.rs:33-64`) | **move the two `ctx_struct!` types down** (plain `f64`/`bool`/`Cell` fields) into `modules/calc_perform_shared.rs`. `setup_flasks.rs` keeps `pub use` | 0 runtime |
| B9 | `pub(crate)` used across crates: `classes/mod.rs:51` `enum StoreMods`, `:67` `ModStore.mods`, `classes/mod_store.rs:41` `arith`, `:152` `ReplaceModInternal`, `classes/mod_db.rs:24` `db_len`, `:33` `db_at` | **widen** to `pub` (`#[doc(hidden)]`). Users: `calc_active_skill.rs:29,86,95`, `calc_perform/actor_b.rs:2271`, `ailments_dispatch.rs:160,162`, ... | 6 lines; no behaviour change |
| B10 | `crate::calcs_facade::calcs` / `crate::calcs` used inside offence (`crit.rs:24`, `hit_dps.rs:29`) and perform (`calc_mirages.rs:25,211,240,1208`, `ailments_dispatch.rs:24`, `actor_a.rs:97`) | **per-crate partial facade**: `calcs_facade.rs` moves whole into perform, where every name it lists exists. offence's `lib.rs` declares a 6-line `calcs_facade::calcs` with what it can see (`actionSpeedMod`, `applyDmgTakenConversion`, `armourReduction`, `armourReductionF`, `hitChance`) | 0 edits in moved files |

Nothing else crosses a crate boundary. The layering DAG and every file's `crate::` paths compile in
the prototype.

### 3.4 The late seam (B3-B6): link-time symbols, no init

The problem: core code must call five functions that live in higher crates, and the following must
keep working unchanged:

* external call sites, such as `configTab.BuildModList(&m)` in pob-app and `mc.call(…)`;
* every test that enters the calc anywhere (`run_segment`, direct module calls).

A registry filled by an `init()` call would require every external entry point to call `init()`,
and would panic wherever one is missed. A trait or generic parameter would change public
signatures.

Design, verified in a 5-crate experiment (core / provider / facade / app / app-without-provider):

```rust
// pob-calc-core/src/late.rs — the one list (macro_export so the providers expand the same list)
#[macro_export]
macro_rules! late_fns { ($cb:ident) => { $cb! {
    BuildOutput(build: &$crate::model::BuildRef) -> ();
    configApply(r: &pob_gamedata::LuaFnRef) -> $crate::late::ConfigApply;
    MiscCalculator_call(this: &$crate::late::MiscCalculator, ov: Rc<Override>, full: Option<bool>) -> Output;
    doActorLifeMana(actor: &$crate::actor::ActorRef) -> ();
    doActorLifeManaReservation(actor: &$crate::actor::ActorRef, addAura: bool) -> ();
} } }
// expands (in core) to: #[cfg(not(test))] extern "Rust" { #[link_name = "pob_calc_late_<name>"] fn …; }
// plus a safe `pub fn <name>(…)` that calls it; under #[cfg(test)] (core's own unit tests) a panicking stub.
// in the provider crate:  pob_calc_core::late_fns!(late_provide);  with  late_provide!  emitting
// #[export_name = "pob_calc_late_<name>"] pub fn …(args) { crate::path::<real fn>(args) }
```

* **Safety**: both sides expand from the same token list, so the signatures cannot differ. Each
  provider body calls the real function, so the real signature is type-checked as well.
  `#[allow(unsafe_code)]` is limited to the two macro-generated modules. Today the workspace has
  `unsafe_code = "deny"` (deny, not forbid). → **needs a CCR/AMENDMENTS decision.**
* **Verified** (experiment run through `cargo.sh`):
  * the app links and runs with rust-lld (the repo config);
  * it links and runs with **GNU ld.bfd**, so rlib order is not a problem;
  * it links and runs in **release + thin LTO + `codegen-units = 1`**;
  * `cargo test -p <core>` links and passes, using the `cfg(test)` stubs;
  * a **wasm32-unknown-unknown** build links, and the `.wasm` has **zero imports**, so the
    symbols resolve internally;
  * a binary that depends on core but not on the provider **fails to link**, natively and on
    wasm32 (`rust-lld: error: undefined symbol: pob_calc_late_BuildOutput`). A missing provider is
    a loud build error, never a silent runtime fault.
  * The prefixed form shown above (`link_name`/`export_name` = `pob_calc_late_<name>`) passed all of
    these checks, and `nm` shows the two `T pob_calc_late_*` symbols in the app. A first run with
    bare `#[no_mangle]` names passed them too.
* **Linking the providers**: rustc links only crates that are used. The facade's `pub use` of every
  sub-crate makes them used. The pob-calc facade is the only crate downstream code depends on.
* **Runtime cost**: a direct call to a Rust-ABI symbol, the same as a normal cross-crate call.

Fallbacks if the CCR is refused, ranked by cost:

1. Explicit registration: core holds a `OnceLock<LateFns>`, and the facade provides
   `pob_calc::init()`. Every binary and test must call it, including pob-app, pob-oracle's testkit
   and pob-ui-oracle's runner. This means changing external code, and a missed entry point panics.
2. Keep `config_options` in core (core 57k lines, peak about +0.4 GB). This removes only B6; B3-B5
   still need a mechanism.

### 3.5 Mirror roots: moved files keep every `crate::` path

Every sub-crate's `lib.rs` rebuilds the part of pob-calc's crate root that its files use, with
re-exports. For example, offence:

```rust
// pob-calc-offence/src/lib.rs  (same #![allow(...)] block as pob-calc's lib.rs)
pub use pob_calc_core::{active_skill, actor, breakdown, cfg, classes, data_fns_types, env, flow,
    global_cache, late, lua_table, model, module_state, prelude, probe, stub};
pub use pob_calc_core::{seg, tbl, unported, ctx_struct};      // `crate::ctx_struct!` (9 uses)
pub mod data_fns;                                             // file moved whole
pub mod modules {
    pub use pob_calc_core::modules::*;                        // calc_tools, common, calc_breakdown, ...
    pub use pob_calc_defence::modules::calc_defence;
    pub mod calc_offence;                                     // -> src/modules/calc_offence/mod.rs
}
pub mod calcs_facade { pub mod calcs {                        // B10
    pub use pob_calc_core::modules::calc_perform_shared::actionSpeedMod;
    pub use pob_calc_defence::modules::calc_defence::{applyDmgTakenConversion, armourReduction,
        armourReductionF, hitChance};
} }
pub use calcs_facade::calcs;
```

As a result, `crate::prelude::*`, `crate::modules::calc_tools as calcLib`, `super::…` and
`crate::ctx_struct!` resolve unchanged in every moved file. The prototype needed **no edits in moved
files** beyond the B-fixes.

### 3.6 Cargo details

* Each sub-crate gets `[lints] workspace = true`, the pob-calc dependency set (pob-lua,
  pob-gamedata, pob-modparser, smallvec, indexmap, ahash, optional serde/serde_json) and
  `probe = ["dep:serde", "dep:serde_json", "<each lower calc crate>/probe"]`.
  pob-calc gets `probe = [all six]/probe`.
* Rule 3 of calc doc §2 (no mlua, no `std::fs` outside non-wasm loaders, wasm32 check) applies to
  every sub-crate.
* `[profile.dev.package."*"]` does not apply to workspace members, so the new crates stay at -O0
  like pob-calc without extra profile lines.
* pob-calc keeps its `[dev-dependencies] mlua` and `tests/hyg1_len.rs`, which uses only
  `pob_calc::lua_table`.

## 4. The facade (`pob-calc` keeps every public path)

```rust
// crates/pob-calc/src/lib.rs (replaces today's module declarations)
pub use pob_calc_core::{active_skill, actor, breakdown, cfg, classes, env, flow, global_cache,
    lua_table, model, module_state, prelude, stub};
pub use pob_calc_core::{seg, tbl, unported, ctx_struct};
pub use pob_calc_offence::data_fns;
pub use pob_calc_perform::{calcs_facade, calcs_facade::calcs};
#[cfg(not(target_arch = "wasm32"))] pub use pob_calc_perform::calcs_facade::try_buildOutput;
pub mod probe {
    pub use pob_calc_core::probe::*;
    #[cfg(feature = "probe")] pub use pob_calc_perform::probe_segments::run_segment;
}
pub mod modules {
    pub use pob_calc_core::modules::*;
    pub use pob_calc_setup::modules::{calc_active_skill, calc_setup};
    pub use pob_calc_config::modules::{config_options, mod_map};
    pub use pob_calc_defence::modules::calc_defence;
    pub use pob_calc_offence::modules::calc_offence;
    pub use pob_calc_perform::modules::{calc_mirages, calc_perform, calc_triggers, calcs, power};
}
pub use env::{EnvRef, Mode}; pub use flow::Flow;
pub use lua_table::{Key, LStr, Output, Table, Val}; pub use model::{Build, BuildRef};
```

**Checked against the actual external uses.** All `pob_calc::` paths in `crates/` and `app/`
outside `crates/pob-calc/src` were collected: **894 references, 282 distinct paths**, in pob-oracle,
pob-app, pob-ui-core, pob-ui-oracle and pob-calc's own `tests/`. Every one maps to exactly one
sub-crate module that the facade re-exports:

| target of the external paths | refs | served by |
|---|---:|---|
| core modules (`lua_table` 54, `classes::*` 123, `env` 61, `model::*` 151, `probe` 84, `stub` 60, `prelude` 43, `global_cache` 34, `active_skill` 30, `modules::{calc_tools,common,calc_format,jewel_funcs}` 46, ...) | 711 | `pub use pob_calc_core::…` |
| root names `Key LStr Table Val BuildRef` | 42 | root re-exports |
| `probe::run_segment` | 27 | facade `probe` module (B7) |
| perform (`modules::{calcs,power,calc_mirages,calc_perform::*,calc_triggers::*}`) | 53 | `modules` re-exports |
| offence (`data_fns::*`, `modules::calc_offence::*`) | 22 | ditto |
| setup (`modules::{calc_setup::*,calc_active_skill}`) | 19 | ditto |
| config (`modules::{config_options::*,mod_map}`) | 11 | ditto |
| `calcs`, `calcs_facade::calcs::{self, MiscCalculator}`, `try_buildOutput` | 7 | perform's `calcs_facade` |
| defence (`modules::calc_defence::{mod,primary}`) | 2 | `modules` re-exports |

These items move, and each one is re-exported at its old path, so external code sees no change:

* `modules::calcs::MiscCalculator` (pob-app `calc/rust.rs:18`, `model.rs:11`): B4;
* `data_fns::{FnRow, SkillHook, …}`: B1;
* `modules::calc_perform::{actionSpeedMod, setup_flasks::{FlaskUpvalues, TinctureUpvalues}}`: B2, B8;
* `modules::config_options::ConfigApply`: B6;
* `probe::run_segment`: B7.

Method-call APIs stay inherent: `ConfigTab::BuildModList` (pob-app, 18 sites) and
`MiscCalculator::call` (pob-app, CA.rs). Type identity is unchanged, because every type has exactly
one definition.

One gap: the `probe!` macro cannot be re-exported at the facade root next to the facade's own
`probe` module, because the name would clash in the type namespace. No code outside probe.rs's own
doc comments invokes `probe!`, so nothing breaks.

## 5. Expected effect (measured)

### 5.1 Monolith baseline

| run (cold, -j3) | peak RSS | rustc wall | note |
|---|---:|---:|---|
| `cargo.sh build -p pob-calc --lib` (outer ptime: 4.18 GB, the same as the rustc) | 4.18 GB | 23.9 s | first build; deps compiled at the same time (cargo run 167.5 s including slot wait and deps) |
| repeats | 4.09 / 4.08 / 4.15 GB | 20.0 / 19.9 / 20.4 s | |
| `--features probe` (as pob-oracle builds it) | 4.05 GB | 20.9 s | |
| `cargo check` (frontend only) | 1.10 GB | 7.2 s | so about 2/3 of the peak is codegen |

`-Ztime-passes` (one run, measurement only, via `RUSTC_BOOTSTRAP=1`; wall 20.3 s, peak 4.12 GB):

| pass | time | RSS after |
|---|---:|---:|
| expand + resolve | 0.4 s | 340 MB |
| type check | 3.2 s | 759 MB |
| MIR borrowck | 3.4 s | 1,122 MB |
| monomorphization collector | 2.9 s | 1,584 MB |
| crate metadata | 3.8 s | 1,632 MB |
| codegen to LLVM IR | 5.5 s | 2,429 MB |
| LLVM passes (parallel workers) | 6.0 s | |

Pass-boundary RSS never exceeds 2.44 GB, but the process peaks at 4.1 GB. The extra ~1.7 GB is
transient and occurs during codegen. It scales with codegen parallelism: with 1 job the peak is
2.93 GB, with 2 jobs 3.62 GB, with 3 jobs 4.1 GB (§6). The likely cause is the LLVM modules in
flight (inferred, not profiled). Smaller crates lower the whole curve (§5.2).

### 5.2 Split (prototype: the same 127k lines as six crates)

| run | max single rustc | max sum of concurrent rustcs | wall |
|---|---:|---:|---:|
| cold, -j3 (3 runs) | 1.37 / 1.40 / 1.37 GB (offence) | 1.80 / 1.80 / 1.81 GB (offence ∥ perform) | 16.6 / 16.5 / 14.5 s |
| cold, -j3, `--features probe` | 1.50 GB | 1.99 GB | 15.1 s |
| cold, `POB_PORT_JOBS=2` | 1.18 GB | 1.60 GB | 18.7 s |

Per crate: see §3.1. Total CPU rises slightly (user time 32-35 s against 29.6 s) because each crate
reloads its dependencies' metadata. Wall time falls, because setup, config and defence compile in
parallel and pipelining overlaps crates.

Typical edits: one statement added at the top of a function body, then the build, then the revert
(both legs measured). Times are whole-cargo-run wall times.

| edit | monolith | split | crates rebuilt (rustc s) |
|---|---|---|---|
| offence `hit_dps.rs` `seg_passHit` | 3.6 / 3.4 s (2nd revert 3.7 s), 1.54-1.56 GB (probe 3.8 / 3.8 s, 1.60 GB) | **1.7 / 1.7 s**, 0.64 GB | offence 1.2, perform 0.7 |
| defence `est_ehp.rs` `seg` | 3.3 / 3.3 s, 1.52 GB | **1.9 / 2.0 s**, 0.59 GB | defence 0.5, offence 1.0, perform 0.6 |
| defence `primary.rs` `seg` | 3.3 / 3.5 s, 1.54 GB | (same crates as est_ehp) | |
| perform `actor_b.rs` | not measured | **0.8 / 0.9 s**, 0.49 GB | perform 0.6-0.8 |
| core `mod_store.rs` `Sum` | 3.4 / 3.4 s, 1.54 GB | 3.8 / 3.8 s, 0.64 GB | all six, 0.3-1.6 each |
| config (touch rebuild) | not measured | 0.4 s | config 0.3 |

In the monolith, every measured edit cost 3.3-3.8 s. That is the fixed incremental overhead (loading and
checking a 127k-line dep graph); codegen of the changed CGU is a small part of it. After the split, an
edit costs that overhead only for the edited crate and the crates above it. Core edits get slightly
slower (+0.4 s), because six small sessions replace one large one.

Not in the prototype:

* the facade crate: about 60 lines of `pub use`, estimated below 0.5 s;
* the probe dispatcher's 170 lines in perform.

Downstream test binaries relink the same way in both layouts.

### 5.3 Compile slots

`cargo.sh` allows `POB_PORT_SLOTS=1` today because one pob-calc rustc reaches 4.1 GB (measured;
the script's comment says 3.5-4). After the split, the whole calc stack of one cargo run peaks at
**≤ 2.0 GB** in total (probe feature, -j3). Two slots (≤ 4.0 GB) then fit within today's single-slot
peak, so **`POB_PORT_SLOTS=2` can be set right away**. Three slots (about 6 GB) need one more
measurement: the compile peaks of the pob-oracle and pob-ui-oracle test binaries, and of
pob-modparser (1.37-1.54 GB at opt-level 1), which run under the same slot. This unit did not
measure them; they do not change with the split. The 5 GB per-run cap keeps about 3 GB of
headroom.

### 5.4 Tests

The split must not lose test coverage:

* The in-crate unit tests live in 10 files: `lua_table`, `eval_mod`, `mod_store`, `passive_spec*`
  (core); `costs_passes`, `dot_combined` (offence); `actor_a`, `actor_b` (perform).
  `scripts/port/full_suite.sh` lists crates by name, so it **must add `pob-calc-core`,
  `pob-calc-offence` and `pob-calc-perform`** (to be safe, all six). Otherwise those tests stop
  running.
* The prototype compiles them (`check --tests --features probe`: clean).
* A run comparing unit-test counts between the monolith and the prototype **did not run**. It waited
  more than an hour for a test-run slot and was stopped (§8).

## 6. Alternatives (measured on the monolith, cold, -j3 unless noted)

| option | peak RSS | rustc wall | stable? | notes |
|---|---:|---:|---|---|
| baseline | 4.08-4.18 GB | 19.9-20.4 s | | |
| `-C codegen-units=16` | 3.91 GB | 19.2 s | yes | −5 %; incremental reuse becomes coarser |
| `debug = 0` for pob-calc (`-C debuginfo=0`) | 3.01 / 3.03 GB | 17.1 / 17.0 s | yes | −27 % peak, −15 % time; pob-calc frames lose file:line in backtraces (panic locations stay) |
| `POB_PORT_JOBS=2` | 3.62 GB | 22.7 s | yes | applies to every crate of the run |
| `POB_PORT_JOBS=1` | 2.93 GB | 32.7 s | yes | +60 % time |
| `debug = 0` + jobs 2 | 2.97 GB | 19.2 s | yes | |
| `debug = 0` + jobs 1 | 2.43 GB | 26.0 s | yes | |
| `CARGO_INCREMENTAL=0` | 2.89 GB | 23.7 s | yes | measured while pob-modparser compiled at the same time, so the peak is likely understated. Makes pob-calc sccache-cacheable (identical sources hit across worktrees), but every edit becomes a full rebuild (about the measured 20-24 s cold time) instead of 3.5 s |
| `-Z threads=4` | 3.79 GB | 21.6 s | **no** (nightly flag; measured via `RUSTC_BOOTSTRAP=1`) | no gain in this measurement |
| `-Z share-generics` | — | — | no | already the default at opt-level 0, so nothing to gain |
| cranelift backend | not measured | | **no**: needs nightly + `rustc-codegen-cranelift-preview`; stable 1.98.1 cannot select a codegen backend | would shorten only the LLVM part (5.5 + 6.0 s of 20 s); no memory data |
| split only `config_options` + `mod_map` (18.5k lines) | not measured as such | | yes | its own crate costs only 0.39 GB / 2.5 s against offence's 1.4 GB / 9 s, so removing it from the monolith saves an estimated ≤ 10-15 % of the peak. Needs the B6 seam anyway |
| split only the biggest files | — | | | the biggest files (`apply_a/b` 17k, `skills_act_int` 3.8k, `probe` 3.4k) are cheap to compile per line (config: 0.39 GB for 18.5k lines). The memory is in the calc modules (offence: 1.4 GB for 27.7k). Splitting files within one crate does not reduce the crate's peak |

None of these comes close to the split: 1.4-1.5 GB per rustc, 1.8-2.0 GB summed, and 0.8-2.0 s for
edits outside core.
`debug = 0` is worth adding on top. It is a one-line profile change, and an estimated −25 % on the
split as well (not measured on the split).

## 7. Migration recipe (keeps git rename detection working)

Order: after INT-1-B merges, and in a window with no open branch that **adds** files under
`crates/pob-calc/src`. Three commits on `port/CSPLIT` (the implementation unit).

**Commit A: "CSPLIT: prep in place".** Inside the monolith, no file moves. The tree builds, and the
full suite and quick corpus pass, so behaviour is unchanged:

1. add `src/late.rs` (the seam, §3.4) and one `late_provide!` invocation, in a new perform-destined
   file `late_provider.rs`. It forwards all five entries; perform depends on config for
   `configApply`. A provider in the same crate works too, so the seam is tested in the monolith
   first;
2. B5, B6, B3: switch the 4 call sites to `crate::late::…`. B4: move `MiscCalculator` to
   `late.rs`, add its inherent `call`, and rename the body in `calcs.rs` to
   `MiscCalculator_call`;
3. B2, B8: add `modules/calc_perform_shared.rs` with `actionSpeedMod` and the two upvalue structs,
   re-export them at the old places, and update 4 `use` lines;
4. B1: add `data_fns_types.rs`, re-export it from `data_fns/mod.rs`, and fix the imports in
   `config_options/mod.rs` and `probe.rs`;
5. B7: move `run_segment` into `probe_segments.rs`, with a temporary `pub use` in `probe.rs`;
6. B9: the 6 widenings.

About 150-250 changed lines.

**Commit B: "CSPLIT: move files (pure renames)"**:

* `git mv crates/pob-calc/src/<path> crates/pob-calc-<crate>/src/<path>` for 98 files (93 of today's
  94, all except `lib.rs`, plus the 5 new files of commit A), using the mapping in §3.1 (scripted
  from that table). `lib.rs` stays.
* **No content edits.** Check with `git show --stat -M HEAD`: every file shows `=> … (100%)`.
* The tree does not build at this commit. Say so in the message (`git bisect skip`).

**Commit C: "CSPLIT: crates, roots, facade"**:

* new `Cargo.toml` × 6 and mirror-root `lib.rs` × 6;
* `pob-calc/src/lib.rs` becomes the facade;
* workspace `members` and `[workspace.dependencies]`;
* trim `modules/mod.rs` in core to the core modules;
* drop the temporary `pub use` from `probe.rs`;
* fix the test paths `pob-oracle/tests/CO-A.rs:391` (`../pob-calc/src/modules/config_options/apply_a.rs`)
  and `SK-A.rs:33` (`include_str!("../../pob-calc/src/data_fns/skills_act_int.rs")`);
* `full_suite.sh` crate list;
* the comment in `cargo.sh`;
* an AMENDMENTS entry with the old → new path map. The 145 mentions of `crates/pob-calc/src` in 59
  docs files can be updated with a sed pass in a follow-up docs commit, or read through the map.

Gates:

* `cargo.sh check -p <each> --target wasm32-unknown-unknown`;
* a wasm32 build of `pob-port-web`, which links and so proves the seam on wasm;
* `cargo.sh test -p pob-calc-core -p pob-calc-offence -p pob-calc-perform --lib`, with the same test
  count as before;
* the pob-oracle quick corpus;
* one full suite run (about 4 h);
* the release power-report timing (§3.2).

Why this keeps merges working:

* Commit B is pure renames, so git's exact-rename detection (cheap and not subject to rename limits)
  pairs every file. `git merge` (ort) then applies an in-flight branch's edits to the new paths.
* Commit A edits ~20 lines in place before any move, so its conflicts look like ordinary conflicts.
* Commit C edits new files and `lib.rs`. It changes only one or a few lines in two moved files
  (`modules/mod.rs`, `probe.rs`); rename detection still pairs them, because the similarity stays
  far above the 50 % threshold.

Two cases rename detection cannot handle:

1. **Code that moved between files** (B2 `actionSpeedMod`, B4 `MiscCalculator`, B7 `run_segment`,
   B8 upvalue structs). A branch that edits those exact lines must re-apply its edit by hand at the
   new location. Check with `git log -L`.
2. **New files** that a branch adds under `crates/pob-calc/src/...`. git's default
   `merge.directoryRenames=conflict` reports them as conflicts. Resolve them with `git mv` into the
   right crate. `src/modules/` splits into several crates, so git's suggested target directory can
   be wrong.

Effort:

* A: about half an agent-day including review;
* B: minutes (scripted);
* C: about half a day, plus one full suite (about 4 h of machine time);
* total: **about 1 unit-day**.

Conflict risk with the named units:

| unit | what it touches | risk | mitigation |
|---|---|---|---|
| INT-1-B (L6 parity fixes in owner files) | any `pob-calc` file, content edits | **medium** while open, **low** after: content edits follow pure renames; real conflicts only on the ~20 lines of commit A and the moved code (B2/B4/B7/B8) | run CSPLIT after INT-1-B merges, as planned. If INT-1-B is still open, it rebases once over A+B+C (renames resolve automatically; check the moved-code list) |
| INT-2 (performance; CA `calcs.rs`, PWR `power.rs`, profiling-driven) | `calcs.rs` (B4 touches `MiscCalculator`'s 20 lines), `power.rs` (none) and release-perf baselines | low (textual), **medium (measurement)**: the split can shift release timings | run INT-2's power-report timing on both sides of the split, and re-baseline INT-2 after it |
| INT-3 (breakdown parity; owner files, CA) | content edits in `calc_breakdown.rs`, offence/defence breakdown code, `calcs.rs` | low | none beyond the general rule; new files under `src/` → directory-rename conflict |

## 8. Not measured / open

* The unit-test count comparison between the monolith (`cargo.sh test -p pob-calc --lib --features
  probe`) and the prototype was **not run**. It waited more than an hour for one of the 3 test-run
  slots, which other agents' oracle runs held, and was stopped. Both test targets compile. The
  implementation unit's gate covers this (§7).
* The compile peak of the pob-oracle and pob-ui-oracle test binaries, which decides 2 against 3
  slots (§5.3).
* Release-mode runtime before and after: an INT-2 gate (§3.2).
* `debug = 0` applied to the split crates.
* The facade crate and the `probe_segments.rs` part of perform were not compiled in the prototype.
  They are re-exports, and `run_segment`'s imports all exist in perform, but the implementation unit
  compiles them first.
* Decisions needed before the implementation unit starts. The implementation unit files one CCR
  for all of them (this unit only adds this document):
  1. the `#[allow(unsafe_code)]` exception for the seam (§3.4). If it is refused, use fallback 1,
     at the cost of an `init()` call in pob-app, pob-oracle and pob-ui-oracle;
  2. code that leaves its owner's file: B2 and B8 from PF-B/PF-C files into
     `modules/calc_perform_shared.rs`, B4 within CA's `calcs.rs` and into core, and B7 out of
     ORC-X's `probe.rs`. The new files and their owners go into calc doc §2.1;
  3. calc doc §2 dependency rule 1 becomes
     `pob-lua ← pob-gamedata ← pob-modparser ← pob-calc-core ← {setup, config, defence}`,
     `defence ← offence`, `{setup, config, defence, offence} ← perform ← pob-calc ← UI/oracle`,
     and rule 3 (wasm32, no mlua) extends to the six crates;
  4. `POB_PORT_SLOTS=2` in `scripts/port/cargo.sh` once the split has landed (§5.3).
