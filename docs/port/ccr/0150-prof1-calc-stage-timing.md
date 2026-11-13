# CCR-0150: PROF1: a stage timing hook in the calc (offence / defence latency)

Unit: **PROF1** (profiling suite; tooling). Status: **proposed**. PROF1 works around it (see §3);
nothing in the calc crates is changed by PROF1. The number was reserved with
`scripts/port/next_ccr.sh`.

## 1. Problem

PROF1's scope asks for recalc latency "broken down where feasible: initEnv / perform / offence /
defence". The calc has no timing seam:

- The app reaches the calc only through `CalcFacade::build_output` / `misc_calculator`
  (crates/pob-app/src/calc/facade.rs:13-22). `RustCalc::build_output` calls
  `power::BuildOutput` (crates/pob-app/src/calc/rust.rs:28-31), which runs
  `calcs::buildOutput(MAIN)`, `calcs::buildOutput(CALCS)` and `getMiscCalculator`
  (crates/pob-calc-perform/src/modules/power.rs:262-295). PROF1 times that call
  (`crates/pob-prof/src/calc_timing.rs`).
- `initEnv` and `perform` are public (crates/pob-calc-setup/src/modules/calc_setup/mod.rs:100,
  crates/pob-calc-perform/src/modules/calc_perform/mod.rs:94), so PROF1 times them from outside
  on a loaded build (`crates/pob-prof/src/stages.rs`).
- `calcs.offence` (crates/pob-calc-offence/src/modules/calc_offence/mod.rs:154) and
  `calcs.defence` / `calcs.buildDefenceEstimations`
  (crates/pob-calc-defence/src/modules/calc_defence/mod.rs:105-119) run only inside `perform`
  (PF-G, `ailments_dispatch::seg`) and `calcFullDPS`, once per actor and active skill. They
  cannot be timed from outside without a hook. Calling them again on a performed env is not the
  same work (they mutate the env).

The only seam-like mechanism in the calc is the `probe` feature (calc doc §9.2), which exports
state and must stay off in measured builds; `late.rs` is a link-time seam for five calls, none
of them a stage.

## 2. Proposed change

A feature-gated, zero-cost-when-off hook in `pob-calc-core`, called at entry and exit of the
stage functions:

```diff
+// crates/pob-calc-core/src/stage_timing.rs (feature `stage-timing`, off by default)
+pub enum Stage { InitEnv, Perform, Offence, Defence, DefenceEstimations, CalcFullDPS }
+thread_local! { static HOOK: Cell<Option<fn(Stage, bool /* enter */)>> = const { Cell::new(None) }; }
+pub fn set_hook(h: Option<fn(Stage, bool)>) { HOOK.with(|c| c.set(h)); }
+#[inline] pub fn mark(s: Stage, enter: bool) { if let Some(h) = HOOK.with(Cell::get) { h(s, enter) } }
```

and in each stage function (one line at entry, one guard for every exit), e.g.

```diff
 pub fn offence(env: &EnvRef, actor: &ActorRef, activeSkill: &ActiveSkillRef) {
+    #[cfg(feature = "stage-timing")]
+    let _t = pob_calc_core::stage_timing::Guard::new(Stage::Offence); // mark(enter) now, mark(exit) on drop
     let mut ctx = OffenceCtx::init(env, actor, activeSkill); // L349-361
```

`pob-calc` re-exports the feature; `pob-prof` enables it and records per-stage inclusive time
and call counts per step (nested calls counted once, by depth). The guard is not Lua
semantics (no `@pob` statement), so it carries no `// L` anchor; it is the same kind of plumbing
as `probe!`. The default build (and the app) compiles it out.

## 3. Work-around in PROF1

- Recalc latency per step from the timing facade (`TimedCalc`, `recalc_ms`, `recalc_max_ms`).
- `initEnv`, `perform`, `buildOutput(MAIN)`, `buildOutput(CALCS)`, `getMiscCalculator` and one
  misc call timed from outside per fixture build (`calc_<build>` entries of every run).
- The offence/defence share from sampling (`scripts/perf/flamegraph.sh`, `pob-prof hotspots`:
  inclusive share of the `calc_offence::` and `calc_defence::{primary,recovery,est_*}` frames).
  Marked `// CCR-0150` in `crates/pob-prof/src/stages.rs`.
