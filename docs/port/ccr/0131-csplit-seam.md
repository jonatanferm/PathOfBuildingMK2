# CCR-0131: CSPLIT, the pob-calc crate split and its link-time seam

Unit: **CSPLIT** (implementation of `docs/port/calc-split-plan.md`, the merged CSPLIT-PLAN).
Status: **applied by CSPLIT** (orchestrator decision (a): the seam and its local
`allow(unsafe_code)` are approved; AMENDMENTS A11 records the result). Number reserved with
`scripts/port/next_ccr.sh CSPLIT-seam`.

`crates/pob-calc` (94 files, 127,021 lines) is split into six crates; `pob-calc` itself becomes a
facade that re-exports every public path, so no crate outside the calc needed a code edit. Files
moved whole (`git mv`, 98 pure renames in one commit), so in-flight branches merge through
rename detection. The plan's §3.3 B-items were applied as listed there, with the deviations in §6
below.

## 1. Crates and dependency rule (calc doc §2 rules 1-3)

Evidence: calc doc §2 rule 1 names one calc crate (`pob-lua ← pob-gamedata ← pob-modparser ←
pob-calc ← pob-ui`).

Proposed rule 1:

```
pob-lua ← pob-gamedata ← pob-modparser ← pob-calc-core ← {pob-calc-setup, pob-calc-config, pob-calc-defence}
pob-calc-defence ← pob-calc-offence
{core, setup, config, defence, offence} ← pob-calc-perform ← pob-calc (facade) ← UI / oracle
```

Rule 3 (no mlua, no `std::fs` outside non-wasm loaders, `cargo check --target
wasm32-unknown-unknown`) applies to all seven calc crates. Crates outside the calc depend on
`pob-calc` only, never on a sub-crate.

| crate | holds (paths under `src/` unchanged) |
|---|---|
| `pob-calc-core` | `lua_table cfg flow stub breakdown global_cache module_state actor active_skill env prelude probe` (minus `run_segment`), `classes/*`, `model/*`, `modules/{mod,calc_tools,common,calc_format,calc_breakdown,mod_tools_env,jewel_funcs,calc_base,calc_perform_shared}.rs`; new `late.rs`, `data_fns_types.rs` |
| `pob-calc-setup` | `modules/calc_setup/*`, `modules/calc_active_skill.rs` |
| `pob-calc-config` | `modules/config_options/*`, `modules/mod_map.rs` |
| `pob-calc-defence` | `modules/calc_defence/*` |
| `pob-calc-offence` | `modules/calc_offence/*`, `data_fns/*` |
| `pob-calc-perform` | `modules/calc_perform/*`, `modules/calc_triggers/*`, `modules/{calc_mirages,calcs,power}.rs`, `calcs_facade.rs`; new `probe_segments.rs`, `late_provider.rs` |
| `pob-calc` | `lib.rs` (re-exports only), `tests/hyg1_len.rs` |

Each sub-crate's `lib.rs` is a *mirror root*: it re-exports the core's root modules and macros, so
the moved files keep every `crate::…` path. offence's root also holds a partial
`calcs_facade::calcs` (the five names it can see; plan B10).

## 2. The late seam (B3-B6) and the `unsafe_code` exception

Evidence: five calls run from core code into higher crates: `model/build_xml.rs:450`
`modules::power::BuildOutput` (Build.lua:695), `model/config_build_mod_list.rs:13`
`modules::config_options::configApply` (ConfigTab.lua:1139), `model/mod.rs:21,218`
`modules::calcs::MiscCalculator` and its `call` (Calcs.lua:100-113), and
`calc_defence/primary.rs:100,109` `doActorLifeMana` / `doActorLifeManaReservation`
(CalcPerform.lua:69/551). All are cold paths (once per load, per config var, per what-if calc, per
actor per defence pass).

Applied: `pob-calc-core/src/late.rs` holds the one list `late_fns!` of the five signatures
(`#[macro_export]`). The core expands it into an `extern "Rust"` declaration of each symbol
`pob_calc_late_<name>` plus a safe `pub fn <name>` that calls it; `pob-calc-perform/src/late_provider.rs`
expands the same list into `#[export_name = "pob_calc_late_<name>"]` functions that call the real
bodies. No registration call, no runtime cost (a direct call), and a binary that links the core
without the provider fails to link (`undefined symbol: pob_calc_late_…`), never at run time. The
declarations are `extern "Rust"` (the Rust ABI), so a Lua error (a panic, D15) unwinds through
the seam as through any other call.

**`unsafe_code` exception** (the workspace has `unsafe_code = "deny"`): `#[allow(unsafe_code)]` is
allowed on exactly two macro-generated modules, both named `seam`: the declaration side in
`late.rs` (the `extern` call) and the definition side in `late_provider.rs` (`#[export_name]`
counts as `unsafe_code`). Nothing else in the calc may use `unsafe`.

**Rules for calc doc §2** (proposed text):

1. Every type in the `late_fns!` list is a full path: `$crate::…` for core types, `::std::…` /
   `::core::…` for std types (`::core::primitive::bool`), `::<crate>::…` for other crates. The
   extern declaration is not type-checked against the provider, and `macro_rules!` resolves a bare
   name at each expansion site, so a bare `Output` could name two types (undefined behaviour). Each
   provider body calls the real function, so the provider side is checked against the real
   signature. Review checks every new entry against this rule.
2. A new late-bound call is added to the list, to `late_provider.rs`'s `imp` module, and needs this
   CCR's kind of sign-off (orchestrator).
3. Only `pob-calc`'s `probe` feature (it enables all six) and the sub-crates' own `probe` features
   (each forwards to every lower calc crate) may enable `pob-calc-core/probe`, plus the middle
   crates' dev-dependency on `pob-calc-perform = { features = ["probe"] }`. `ctx_struct!`'s
   `cfg(feature = "probe")` is evaluated in the invoking crate, and `ProbeState::probe_locals` is a
   required method under the core's `probe`: a crate with `probe` off next to a core with `probe` on
   fails with E0046.
4. The unit-test binaries of `pob-calc-setup`, `-config`, `-defence` and `-offence` link the
   provider: each has `[dev-dependencies] pob-calc-perform = { workspace = true, features =
   ["probe"] }`, `#[cfg(test)] extern crate pob_calc_perform as _;` in its `lib.rs` and
   `[lib] doctest = false`; a future `tests/*.rs` of these crates needs `use pob_calc_perform as
   _;`. The core's own unit tests get panicking `cfg(test)` stubs instead (a dev-dependency on the
   provider would link a second core).

## 3. Code that left its owner's file (B1, B2, B4, B7, B8)

| item | from (owner) | to |
|---|---|---|
| B1 `SkillHook`, `SkillFunc`, `ExplosiveArrowFunc`, `SkillFuncImpl`, `FnRow` | `data_fns/mod.rs` (U0) | core `data_fns_types.rs` (U0, frozen); `data_fns/mod.rs` re-exports them |
| B2 `actionSpeedMod` (`@pob Modules/CalcPerform.lua:1141-1151`, marker moved with it) | `calc_perform/actor_b.rs` (PF-B) | core `modules/calc_perform_shared.rs`; `actor_b.rs` re-exports it |
| B8 `FlaskUpvalues`, `TinctureUpvalues` | `calc_perform/setup_flasks.rs` (PF-C) | core `modules/calc_perform_shared.rs` (frozen, CCR-0079 §1); `setup_flasks.rs` re-exports them |
| B4 `MiscCalculator` (struct + inherent `call`) | `modules/calcs.rs` (CA) | core `late.rs`; the closure body stays in `calcs.rs` as `misc_calculator_call` (Rust-only name, snake_case: the Lua closure has none; STYLE §3.1 rule 1); `calcs.rs` re-exports the struct |
| B6 `ConfigApply` alias | `config_options/mod.rs` (U0) | core `late.rs`; `config_options` re-exports it |
| B7 `run_segment` | `probe.rs` (ORC-X) | perform `probe_segments.rs` (ORC-X); `pob_calc::probe::run_segment` re-exports it |

Owners for calc doc §2.1: `late.rs`, `late_provider.rs` [CSPLIT, frozen: change through a CCR];
`data_fns_types.rs` [U0]; `modules/calc_perform_shared.rs` [PF-B] (its two upvalue structs stay
frozen); `probe_segments.rs` [ORC-X]. The new `lib.rs` mirror roots and the facade [CSPLIT].

B9 widenings `pub(crate)` → `pub` (one token each, behaviour unchanged): `classes/mod.rs`
`StoreMods`, `ModStore.mods`; `classes/mod_store.rs` `arith`, `ReplaceModInternal`;
`classes/mod_db.rs` `db_len`, `db_at`; the 19 segment functions `run_segment` calls (CS1
`init_env_base::seg`, CS2 `init_env_items::seg`, CS3 `init_env_skills::seg`, DF-B..DF-G `seg`,
OF-B `seg_skillTypeStats`, OF-C `seg_costsAndPasses`, OF-D `seg_speed`, OF-E `seg_passCrit`, OF-F
`seg_passHit`/`seg_combinePasses`, OF-G `seg_ailmentPrelude`/`seg_bleedPoison`, OF-H
`seg_igniteNonDmgImpale`, OF-I `seg_dotCombined`); `probe.rs` `ctx_fill`, `ctx_out`. The
compiler confirmed the list is complete (`check -p pob-calc-perform --features probe` clean on
the first build after the move).

## 4. Paths that changed

External `pob_calc::…` paths: none (the facade re-exports all of them; every crate that depends on
pob-calc compiles with `--tests` unchanged). Source paths: `crates/pob-calc/src/<p>` →
`crates/pob-calc-<crate>/src/<p>` by the table in §1. Four pob-oracle tests read calc sources by
path and were updated: `CO-A.rs:391`, `CO-B.rs:506` (`pob-calc-config/src/modules/config_options/`),
`SK-A.rs:33`, `SK-B.rs:84` (`pob-calc-offence/src/data_fns/`). The 145 mentions of
`crates/pob-calc/src` in `docs/` are not rewritten: read them through the map in AMENDMENTS A11.

## 5. Process

* `scripts/port/full_suite.sh` lists the six crates (their unit tests: core 32, offence 4,
  perform 3; setup, config, defence 0), the same 39 tests as the monolith.
* `POB_PORT_SLOTS` stays 1 (orchestrator decision; CSPLIT reports the run-level peaks).

## 6. Deviations from the plan

* `late_fns!` is also re-exported from each mirror root (`crate::late_fns!` in the moved
  `late_provider.rs`, as `crate::ctx_struct!` elsewhere).
* `pob-calc-core` also has `[lib] doctest = false` (its doc tests would link no provider; there
  are none, as in the monolith: 0 doc tests).
* primary.rs and helpers_setup.rs import `actionSpeedMod` from `calc_perform_shared` and call it
  unqualified (two call lines edited) instead of aliasing `calc_perform_shared` as `actor_b` /
  `calc_perform_b`.
* `pob-calc`'s facade keeps `pob-lua` as a dev-dependency (`tests/hyg1_len.rs`).
* The `cargo.sh` comment on the slot count (plan §7) is left to its owner (TP3).

## 7. Verification and measurements (CSPLIT, aarch64, 8 cores, shared machine)

Seam gates on the real crates (plan §7):

* rust-lld (repo config): `pob-port-desktop` links; `nm` shows the five `T pob_calc_late_*`
  symbols. `pob-app` unit tests 58/58.
* GNU ld.bfd (`RUSTFLAGS="-C link-arg=-fuse-ld=bfd"`, separate target dir; `.comment` has no LLD
  entry): `pob-port-desktop` links with the five `T pob_calc_late_*`; `pob-app` unit tests 58/58.
* wasm32: `check --target wasm32-unknown-unknown` of the seven calc crates (only power.rs's
  existing 7 dead-code warnings); `build --target wasm32-unknown-unknown -p pob-port-web` links;
  its 775 imports are all wasm-bindgen's, none `pob_calc_late_*`.
* Middle-crate test binaries: setup 0, config 0, defence 0, offence 4, perform 3, core 32 (stubs)
  = the monolith's 39 unit tests, same names. A temporary defence unit test that takes
  `late::doActorLifeMana` / `late::BuildOutput` as fn pointers links with the dev-dependency
  remedy (symbols present) and fails without it (`rust-lld: error: undefined symbol:
  pob_calc_late_BuildOutput`, `…doActorLifeMana`).
* Behaviour: a probe binary (load each of the 57 corpus builds with `loadBuildFromXML`, which
  runs `BuildOutput`, `ConfigTab:BuildModList` and the defence pass, then
  `MiscCalculator::call`; digest of every main-output and misc-output leaf as exact f64 bits) gives
  identical digests for the pre-split monolith (rust-port at 7ce98f3, dev), the split (dev) and the
  split in release (thin LTO, `codegen-units = 1`).
* `calc_depgraph.py check`: 946 file edges, 0 wrong-way; `external-paths`: 905 `pob_calc::`
  references / 281 distinct paths outside the calc, all served by the facade.

Oracle acceptance on the split tree (full corpus, `POB_FULL_SUITE=1`, one binary at a time through
`cargo.sh`): CA `ca_l6_corpus` headline 741/741, all keys 45,954/45,954, CALCS lists 171/171,
used-tracking 513/513, misc calculator 1,248/1,248, 0 Rust errors; `ca_l5_post` CALCS lists 57/57
clean; `ca_d13_fixed_point` 62/62 fixed points, 0 differ. PWR 108,820/108,820 power values exact
(15 runs over 4 builds; coroutine and parallel runs exact). PF-B 1,238/1,239 clean
(`actionSpeedMod` 399/399; the other one is the documented not-gated itemsTab-slots case of
allflame_exp_039, as before). AS 5,015 cases, 0 MISMATCH; L4 56/56. CS2 L4 1,104/1,104 pass, 0
fail, restarts 6/6 identical; overrides 0 failures. Provenance `check`: 2,159 `@pob`, 215
`@pob-skip`, 226 `@pob-file` (one more: `calc_perform_shared.rs`), 0 errors; `coverage` is
unchanged from rust-port (0 uncovered, the same 421 lines covered twice by pob-app's tab code);
`locals_audit.py --check` ok. Further pob-oracle binaries (full corpus): CO-A 2/2 and CO-B 2/2
(they read `apply_{a,b}.rs` at the new path), SK-A 2/2 and SK-B 2/2 (sources at the new path), ORC-X 6/6, DF-C, OF-G, OF-I, PF-E, PF-D 2/2 (L4 through
`run_segment`), MS-B 2/2, BM-B 5/5, oracle_harness 2/2, u0_smoke; U0 5/6, its
`provenance_check_and_coverage` failing on the same pre-existing coverage duplicates as rust-port.
A simulated pre-split branch that edits `hit_dps.rs`,
`mod_store.rs`, `probe.rs` and `actor_b.rs` merges cleanly (`merge-tree`), each edit at the new
path.

Compile memory (dev profile, `CARGO_BUILD_JOBS=3`, peak RSS per rustc from `wait4`):

| crate | cold peak / rustc wall | with `debug = 0` |
|---|---|---|
| pob-calc-offence | 1.12 GB / 10-15 s | 0.94 GB |
| pob-calc-perform | 0.77-0.98 GB / 4.5-9 s | 0.68 GB |
| pob-calc-core | 0.70-0.74 GB / 9.5-11.5 s | 0.63-0.68 GB |
| pob-calc-defence | 0.54 GB / 3.7-4.8 s | 0.47-0.48 GB |
| pob-calc-config | 0.37 GB / 2.4-2.9 s | 0.33-0.34 GB |
| pob-calc-setup | 0.35 GB / 2.3-3.2 s | 0.32-0.33 GB |
| pob-calc (facade) | 0.09 GB / < 0.1 s | 0.09 GB |
| (monolith pob-calc, plan §5.1) | 4.08-4.18 GB / 20 s | 3.0 GB |

Run-level peaks (sum over every rustc/linker of the run, 0.2 s sampling; fully cold target dir,
registry crates from sccache):

| run | sum RSS | sum PSS | at the peak | `debug = 0` (calc crates) RSS / PSS |
|---|---|---|---|---|
| `cargo.sh test -p pob-oracle --no-run` | 2.72 GB | 2.19 GB | 3 rust-lld (1.03 + 0.71 + 0.50) | 2.41 / 2.00 GB |
| `cargo.sh test -p pob-ui-oracle --no-run` | 4.12 GB | 2.69 GB | 3 rust-lld (1.58 + 1.02 + 0.89) | 4.06 / 2.79 GB |
| `cargo.sh build -p pob-calc --lib` (calc + modparser/gamedata cold) | 2.51 GB | 2.24 GB | modparser 1.23 + core 0.65 + gamedata 0.57 | 2.55 / 2.35 GB |
| middle-crate `test -p pob-calc-setup --lib --no-run` (builds perform with probe) | 1.67 GB | 1.61 GB | offence 1.15 + perform 0.48 | |

The largest single process is now pob-modparser (1.28 GB) or a pob-ui-oracle test link
(1.58-1.63 GB); the calc's largest is offence at 1.12 GB. The peaks of both cold test builds
are in the link phase, which the split does not change (plan §5.3).

Edit → rebuild (`cargo.sh build -p pob-calc --lib`, one statement added at the top of the
function, then reverted; whole cargo run):

| edit | split | crates rebuilt (rustc peak) | monolith (plan §5.2) |
|---|---|---|---|
| offence `hit_dps.rs` `seg_passHit` | 1.7 / 1.7 s (`debug = 0`: 1.7 / 1.6 s) | offence 0.62 GB, perform 0.45 GB | 3.4-3.7 s, 1.54-1.56 GB |
| defence `est_ehp.rs` `seg` | 2.0 / 1.8 s | defence 0.35, offence 0.56, perform 0.45 GB | 3.3 s, 1.52 GB |
| perform `actor_b.rs` `initMinionModDB` | 0.8 / 0.8 s | perform 0.47 GB | |
| core `mod_store.rs` `Sum` | 3.7 / 3.6 s | all six, ≤ 0.61 GB each | 3.4 s, 1.53-1.54 GB |
