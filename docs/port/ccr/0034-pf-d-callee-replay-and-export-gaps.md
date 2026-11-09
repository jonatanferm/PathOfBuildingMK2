# CCR-0034: PF-D — replaying callees inside a segment, and three export/schema gaps

Status: proposed by PF-D (2026-11-09)

(Number chosen as the next free one in `docs/port/ccr/` on the PF-D branch; renumber on merge if
another unit took 0034.)

## 1. Segment L4 needs callee replay *inside* the segment (no contract for it)

Calc doc §9.2 "Replay" and §11 ("Units in Waves 3-4 seed from the oracle and replay their
callees") assume a segment's not-yet-ported callees can be swapped for their recorded deltas, and
`pob_oracle::replay::Replay` applies such a delta. But nothing lets the test reach *into* a running
segment: `pob_calc::probe::run_segment` calls the Rust segment, which calls the Rust callee directly,
and an `unported!` callee just returns `Default` (no state change). For PF-D (CalcPerform.lua
1943-2307) the callees are PF-A's `doActorAttribsConditions` (1944, 1964), `doActorLifeMana` (1945),
`doActorLifeManaReservation` (2092), `getCachedOutputValue` (2281, 2288, 2299), PF-B's
`addMinionModifiers` (1957), `doActorCharges` (2191) and PF-C's `mergeTinctures` (2095); without
replay the L4 diff is dominated by their missing writes (e.g. every `output.Str/Dex/Int/Life/Mana`).
Also, line probes exist only at segment boundaries, so there are no `:in`/`:out` dumps around these
calls (only `getCachedOutputValue` is a function probe).

**Workaround (PF-D, marked `CCR-0034`):**

* `crates/pob-calc/src/modules/calc_perform/pools_charges.rs`: every such call goes through
  `callee(site, || …)` / `cached_output(…)`; with feature `probe` a thread-local hook
  (`pools_charges::callee_replay::set`) may answer the call instead (returning the callee's results).
  Without an installed hook (and always without `probe`) the Rust callee runs.
* `crates/pob-oracle/corpus/PF-D/stages.toml`: `docs/port/stages.toml` plus line probes `pfd:<line>`
  before and after each call site; `tests/PF-D.rs` maps each site to its `(before, after)` dumps and
  answers the hook with `Replay::apply_report`.

**Proposed diff** (ORC-X / U0, so every segment unit shares one mechanism):

```diff
 // crates/pob-calc/src/probe.rs (feature "probe")
+/// Callee replay inside a segment: `callee!("<site>", expr)` evaluates `expr` unless the installed
+/// hook answers `<site>` (then the hook's return values are used).
+pub type CalleeHook = Box<dyn FnMut(&str) -> Option<Vec<Val>>>;
+pub fn set_callee_hook(h: Option<CalleeHook>) -> Option<CalleeHook>;
+#[macro_export] macro_rules! callee { ($site:literal, $e:expr) => { … } }
 // crates/pob-oracle/src/testkit.rs
+/// `segment_diff` with call-site replays: `sites` maps a `callee!` site to the stages.toml line
+/// probes fired before / after it.
+pub fn segment_diff_with_replay(build: &Path, stage_in: &str, stage_out: &str,
+                                sites: &[(&str, &str, &str)]) -> Vec<String>;
 # docs/port/stages.toml: per-call-site line probes for the callees of each segment
```

## 2. `SkillLevel.cost`: nil vs `{ }` is lost

CalcPerform.lua:1985/1990 test `activeSkill.activeEffect.grantedEffectLevel.cost` for truthiness.
`pob_gamedata::SkillLevel.cost` is a `Vec<(Name, f64)>`, empty both for a level without `cost` and
for `cost = { }` (Data/Skills/minion.lua: 5 levels, spectre.lua: 18). PF-D reads an empty `cost` as
absent (`has_cost`); this differs from PoB only for those minion/spectre levels with
`ManaCostGainAsReservation` / `LifeCostGainAsReservation`.

**Proposed diff:** `pub cost: Option<Vec<(Name, f64)>>` in `schema/skills.rs` (and the
deserializer sets `Some` when the key is present).

## 3. Values the exporter cannot see (test-side adaptations in `tests/PF-D.rs`)

a. **Typed field assigned a default after injection.** `env.buffs`, `env.minionBuffs`,
   `env.debuffs` are nil at `perform:1943`; injection marks them absent, and
   `probe::struct_members` drops an absent field that holds a default value (an empty table), so
   `env.buffs = buffs` (CalcPerform.lua:2195/2198/2200) exports as missing. The test calls
   `probe::unmark_absent` for those three. *Proposal:* the exporter treats an absent-marked
   `Field` as present once its handle differs from the one injection left (record the injected
   handle's pointer instead of a bare "absent" flag).

b. **Closures stored in calc tables.** `row.sourceNameTooltip = function(tooltip)
   env.build.itemsTab:AddItemTooltip(tooltip, item, reqSource.sourceSlot) end`
   (CalcPerform.lua:2139-2141) is ported as a value `pools_charges::SourceNameTooltip
   { build: Weak<Build>, item, sourceSlot }` that the UI can call. The exporter writes unknown
   `Val::Obj`s as `{"$userdata": true}`, and a replay (which writes the whole exported live state
   back) turns them into that marker. The test registers each such value as the foreign marker
   `{"$fn": "Modules/CalcPerform.lua:2139-2141"}` (before every replay and before the diff).
   *Proposal:* a `pob_calc::LuaClosure` trait (`fn src(&self) -> &'static str`) that the exporter
   recognises and writes as `{"$fn": src}`, and that replay keeps (does not overwrite) when the
   dumped value is the same `$fn`.

c. **Build model fields.** `env.spec.build.spectreList` (CalcPerform.lua:2222) lives on the build
   model, which dumps do not carry and BM-A's loader does not fill yet; the test fills it from the
   XML as Build.lua:987-993 does. Also, the model's default `PassiveSpec` has no `build`
   back-pointer, so PF-D reads `env.spec.build` and falls back to `env.build` (the same build in
   PoB: CalcSetup.lua:501, PassiveSpec.lua:16).

## 4. Note (no change requested)

CalcPerform.lua:1951-1952 `modLib.setSource(copyTable(mod, true), …)` re-sources `mod.value.mod`
through the shallow copy, i.e. the original EnemyModifier payload's nested mod (a shared-mutation
site not in contracts.md#mod-mutation, like CCR-0014's `mergeKeystones`). The port re-sources a
copy and leaves the stored mod alone; this differs only when an EnemyModifier wraps a mod whose
value itself carries a `{ mod = … }` with another source.

## 5. `tests/u0_smoke.rs` cannot stay green once segments are faithful (U0)

`build_output_on_stubs` runs `buildOutput` on slice5 with every upstream segment stubbed and
expects the whole of `calcs.perform` to run (it asserts the PF-G stub site `CalcPerform.lua:3497-3990`
is reached). With PF-D ported, perform now stops at CalcPerform.lua:2206
(`env.partyMembers["Aura"]`): `env.partyMembers` is assigned at 1310, inside PF-C's still-stubbed
segment, so it is nil and PoB's own code would raise "attempt to index field 'partyMembers'" too.
PF-D keeps the Lua semantics (raise) rather than tolerate state an unported segment failed to
create. **Proposal:** `build_output_on_stubs` accepts a `LuaError` raised inside a *ported* segment
whose input comes from a stubbed one (or drops the PF-G expectation) until PF-C lands; the
integrator should expect this test to fail on `rust-port` after merging PF-D and before PF-C.
