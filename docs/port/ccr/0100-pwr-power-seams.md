# CCR-0100: PWR (CalcsTab power report) contract notes and seams

Status: proposed by PWR (port/PWR, 2026-11-10)

PWR ports `Classes/CalcsTab.lua` 441-773 and `data.powerStatList.GetFromOutput` with its
`transform` closures (Data.lua 131, 164-168, 186-212) in `crates/pob-calc/src/modules/power.rs`.
The frozen stub (U0) is kept except for one return type (§2). Everything else is an addition in
the owned file.

## 1. The `calcsTab` / `build` fields the frozen structs have no slot for

Evidence: CalcsTab.lua reads and writes `self.powerStat` (498, 538, 547, 629, 689, 693, 724),
`self.powerMax` (478, 511-512, 737), `self.nodePowerMaxDepth` (570, 588, 608),
`self.powerBuilder` (479-487), `self.powerBuilderInitialized` (738) and
`self.miscCalculator = { miscCalcFunc, miscCalcBase }` (471, 761); Build.lua:20-21 declares
`build.powerBuilderProgressCallback` / `build.powerBuilderCallback` (read at 488-489, 656-657).
`model::CalcsTab` (model/mod.rs, U0) has `miscCalculator: Field<Option<Rc<MiscCalculator>>>` (the
function only, not the base output) and none of the others; `model::Build` has no callback slots.

Work-around (in place, `// CCR-0100` in power.rs's module doc): the fields live in BM-A's build
side table (`build_xml::build_extra(build)`), under their Lua names, in a `calcsTab` sub-table
(`miscCalculatorBase` for `self.miscCalculator[2]`); the callbacks in the build side table. Typed
accessors in `power.rs`: `powerStat` / `set_powerStat`, `powerMax`, `nodePowerMaxDepth` /
`set_nodePowerMaxDepth`, `powerBuilderInitialized`, `powerBuilderRunning`,
`set_miscCalculator`, `set_powerBuilderCallback`, `set_powerBuilderProgressCallback`,
`set_powerOptions`. U26 (TreeTab power), U40 and CompareTab use these.

Proposed diff (optional; the accessors keep working if the fields are typed later):

```rust
// model/mod.rs
pub struct CalcsTab {
    …
    pub miscCalculator: Field<Option<Rc<MiscCalculator>>>,
+   pub miscCalculatorBase: Field<Option<Output>>,
+   pub powerStat: Field<Option<Table>>,          // a data.powerStatList entry as power::powerStat_table builds it
+   pub powerMax: Field<Option<Table>>,
+   pub nodePowerMaxDepth: Cell<Option<f64>>,
+   pub powerBuilderInitialized: Cell<bool>,
}
```

`powerStat` is a `Table` (not `&PowerStat`) because callers pass ad-hoc `{ stat = … }` tables too
(CompareTab.lua:974, TradeQuery.lua:812-814); `transform` is `Val::Obj(Rc<LuaFnRef>)`, the
representation ItemsTab.lua:1309 (BM-A) already uses.

## 2. `CalculateCombinedOffDefStat` returns both results

Evidence: CalcsTab.lua:757 `return dpsIncr / modifiedDps, defence`; 548 assigns both
(`power.offence, power.defence = …`). The frozen stub returns `f64`.

Diff (applied in the owned file; no other caller exists):

```diff
-pub fn CalculateCombinedOffDefStat(original: &Output, modified: &Output) -> f64;
+pub fn CalculateCombinedOffDefStat(original: &Output, modified: &Output) -> (f64, f64); // (offence, defence)
```

## 3. The coroutine

`PowerBuilder(state, build, options) -> Option<()>` (frozen) is one `coroutine.resume`: `Some(())`
when the body yielded (at 515, or after a counted node once `GetTime() - start > 100`),
`None` when it returned. The body's locals live in `state.extra` (an opaque `$iter` object), so
`PowerState` keeps its frozen fields. Additions: `PowerBuilder_run(build, options)` is the body
called outside a coroutine (`coroutine.running()` nil: it never yields; Lua errors propagate);
`GetTime()` is a host hook (`set_GetTime`; desktop default: a monotonic ms clock, wasm default 0);
`launch.devMode` is `launch_devMode()` / `set_launch_devMode` (CalcsTab.lua:483 re-raises only
in dev mode; otherwise the error just ends the coroutine, as in a release PoB).

## 4. Parallel mode (D12)

`PowerOptions { parallel: true }` (opt-in; `set_powerOptions` for `BuildPower`): when the body
leaves its first yield it runs itself once as a dry run on placeholder outputs, recording the calc
calls in order (which calls the body makes depends only on the tree, the modKey cache and
`powerStat`, never on an output value), runs them on up to 4 worker threads, each with its own
`Build` loaded from `build:SaveDB` XML (`loadBuildFromXML`: one GlobalCache and tree cache per
thread), and then runs the body on the results. A call sequence that changes during the run
(`powerStat` changed between frames) falls back to the live calculator for the rest. Results can
differ from the sequential run where the calc reads GlobalCache entries the sequential order left
behind (D11); `crates/pob-oracle/tests/PWR.rs` `pwr_l6_report` reports the divergences.

## 5. `power::calc_replay` (feature `probe` only; test seam)

`calc_replay::set(Some(hook))` answers `calcFunc(override, useFullDPS)` inside the PowerBuilder
body; unset (the product, and without `probe`), the misc calculator runs. Used by PWR.rs's L5
replay of PoB's recorded calc outputs (the gate), per AMENDMENTS "test seams".

## 6. Not done here

`crates/pob-oracle/tests/CA.rs` keeps its `calcsTab_BuildOutput` scaffold (CA's file; FOLLOWUPS
"CA" item 2). It catches the CALCS stage separately, which the real `BuildOutput` does not (in
PoB an error there aborts the whole call); CA can switch to `power::BuildOutput` when it no longer
needs that.
