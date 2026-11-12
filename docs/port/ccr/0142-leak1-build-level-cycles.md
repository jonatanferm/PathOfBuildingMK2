# CCR-0142: LEAK1: two build-level `Rc` cycles keep a dropped build (and its envs) alive

Unit: **LEAK1** (found while applying CCR-0135; not a frozen-contract change made here). Status:
**pending** (a local teardown in the web calc worker is applied, §3 A; the general fix, §3 B, is
for the orchestrator / owners of the build model and the app). Number reserved with
`scripts/port/next_ccr.sh`.

## 1. Problem

calc doc §3.3 rule 4: "`Rc` cycles are forbidden." After CCR-0135 no cycle keeps an **env** alive
once its owners are gone (`crates/pob-calc/tests/leak1_cycles.rs`: a dropped CALCULATOR env, and
every env a recalc replaces, incl. the `GlobalCache` entries', free their whole graph). Two cycles
remain at the level of the **build**:

1. **Build ↔ its envs.** `Env.build: BuildRef` (`pob-calc-core/src/env.rs:82`, Lua `env.build =
   build`, CalcSetup.lua `initEnv`) and `MiscCalculator.build: BuildRef`
   (`pob-calc-core/src/late.rs:40`, the closure of `getMiscCalculator`, Calcs.lua:100-113) are
   strong; the build owns them through `calcsTab.mainEnv`, `calcsTab.calcsEnv`,
   `calcsTab.miscCalculator` (`pob-calc-core/src/model/mod.rs:214-218`). A recalc replaces the
   envs, so it does not leak; but a build that is dropped is never freed (strong count 3 = those
   three edges), nor are its two envs.
2. **Socket group ↔ its display skills.** `SocketGroup.displaySkillList` /
   `displaySkillListCalcs: Option<Shared<Vec<ActiveSkillRef>>>` (`model/mod.rs:130-131`) hold the
   group's skills strongly, and each skill holds `ActiveSkill.socketGroup: Option<SocketGroupRef>`
   (`active_skill.rs:85`, Lua `activeSkill.socketGroup = socketGroup`, CalcActiveSkill.lua:89)
   strongly. A recalc replaces the lists, so it does not leak; a socket group that leaves the
   model (deleted in the Skills tab, or its build dropped) keeps itself, its last display skills
   and everything they reach (minion actors, mod lists) alive.

Lua's GC collects both. Measured (`leak1_cycles.rs`, `build_teardown_frees_everything_ccr0142`;
`loadBuildFromXML`, then drop the build and wipe the `GlobalCache`; objects = envs, actors,
skills and actor/skill mod stores reachable from `mainEnv` and `calcsEnv`):

| build | (a) drop as is | (b) after clearing the 3 `calcsTab` edges | (c) and every group's display lists |
|---|---|---|---|
| allflame_exp_039 | build alive, 176/176 survive | build freed, 84/176 survive | 0 |
| allflame_exp_007 | build alive, 252/252 | freed, 122/252 | 0 |
| allflame_exp_001 | build alive, 136/136 | freed, 64/136 | 0 |
| allflame_exp_000 | build alive, 100/100 | freed, 46/100 | 0 |

Where it matters:

- **The web calc worker** (`pob-app/src/calc/worker_web.rs` `run_request`) loads a fresh build model
  per request (`loadBuildModelFromXML`, `BuildOutput`) and drops it: before §3 A every request
  (every build edit in the web app) leaked one whole build with its two envs.
- **The desktop app** drops a build when another one is opened (`pob-app/src/build/init.rs:755`,
  `cx.app_mut().build.take()`): one build and its envs per switch.
- **Deleting a socket group** leaks that group's last display skills.

## 2. Why not `Weak`

`Env.build` is read everywhere in the calc (`env.build.<tab>` …); a cached env (`GlobalCache`)
can outlive its build's current `mainEnv` but never the build itself in Lua, so a `Weak` would be
safe in principle, but the retype touches hundreds of reads in every calc crate. A `Weak`
`ActiveSkill.socketGroup` is not safe: a skill of a cached env keeps reading its group after
the group left the model (Lua keeps the table alive). So the proposal is an explicit teardown.

## 3. Proposed change

A. **Applied by LEAK1** (local, `// CCR-0142`): `run_request` breaks both cycles after it has read
   the stats, then wipes the `GlobalCache` (its entries' envs point back to the build too):

```rust
build.calcsTab.mainEnv.set(None);
build.calcsTab.calcsEnv.set(None);
build.calcsTab.miscCalculator.set(None);
for group in build.skillsTab.socketGroupList.get().snapshot() {
    group.displaySkillList.set(None);
    group.displaySkillListCalcs.set(None);
}
pob_calc::modules::common::wipeGlobalCache();
```

   Gate: `leak1_cycles.rs` variant (c) (0 survivors, build freed, on the four builds above).

B. **Proposed** (owners: build model U0/BM-A, app U-shell, Skills tab): a Rust-only
   `Build::teardown(&self)` in `pob-calc-core/src/model/` doing A's body (the `calcsTab` edges,
   every socket group's display lists, `wipeGlobalCache`), called (1) by `run_request` instead of
   A's inline copy, (2) by the app before it drops a `BuildState` (`build/init.rs:755`, and the
   app's other build-closing paths), and (3) for one group by the Skills tab when it removes a
   socket group from `socketGroupList` (clear that group's `displaySkillList(Calcs)`). calc doc
   §3.3 rule 4 gains: "Exception (CCR-0142): `Env.build`, `MiscCalculator.build` and
   `ActiveSkill.socketGroup` are strong (Lua aliases); whoever drops a build or a socket group
   calls `Build::teardown` / clears the group's display lists first."

## 4. Validation

`leak1_cycles.rs` (3 tests, all pass on rust-port + LEAK1). B needs an app-level check (open two
builds in a row; the first `Build`'s `Weak` must be dead).
