# Rust port: status and how to resume

## Current status (2026-11-12, CCRDOC)

**Every planned unit row is merged into `rust-port`** (the integration branch). This covers every
row of calc doc §11.1, including INT-1..INT-4, and every row of ui doc §12, including U53 (cut-over)
and the optional U52 and U54. The only exception is U51. The command-line import of
Main.lua:71-83 is ported (FUP5, FOLLOWUPS "CCRDOC" 12).

- **Calc:** no Lua at runtime (`cargo tree -e normal | grep -c mlua` = 0 for pob-calc, pob-app,
  pob-desktop and pob-web; AMENDMENTS A13). Parity, last measured by FUP4 (FOLLOWUPS "FUP4"):
  all57 L6 headline 741/741, all keys 45,954/45,954, 0 Rust errors, D13 62/62. The fresh
  100-build corpus gave all keys 85,603/85,603 and D13 100/100.
- **UI:** `pob-app` on `RustCalc` is the product (`app/pob-desktop`, `app/pob-web`). U53's e2e
  run (27 runs, 1,668,755 Lua draw ops) had 0 T3 divergences, and T4 matched (33 pairs). Every
  unit harness u16-u52 runs on the Rust calc (CCR-0145 §1).
- **Contracts:** every CCR file in `docs/port/ccr/` has a final status: applied, rejected (not
  built, with the reason) or superseded. CCRDOC decided the last 52 (0043-0145, AMENDMENTS A14).
  calc doc, ui doc, contracts.md and STYLE.md describe the code as built.

**Deferred by user decision (2026-11-12):**

- **U51 Trade** (TradeQuery and the related classes; ui doc §12.7). Its two ItemsTab hooks still raise
  "… is not ported yet (U51)". U27 and U49 carry the local copies they need (`FetchLeagues`,
  TradeHelpers).
- **Update checker:** stubbed, and the "Check for Update" button is hidden (FOLLOWUPS
  "BRAND" 1). The port's version is the `VERSION` file (`YYYY.MM.DD.N`).

**Pending per AMENDMENTS A5:** T6, the SSIM ≥ 0.995 check against real SimpleGraphic under Wine
on 15 scenes (U53 row). Wine is not installed (FOLLOWUPS "U53" 1).

**Open follow-ups** (`docs/port/FOLLOWUPS.md`):

- "CCRDOC" 1-12 are code and doc differences found while deciding the CCRs. FUP5 closed 1, 2,
  3, 7, 10 and 12 (build list rebuild on Options Save, the stale Import-to-new-build frame, the
  POST Content-Type, the dropped-build and deleted-group leak, the range slider knob, the
  command-line build link). Still open in the product: the comparison build's item/jewel tooltips
  and config tooltips are wrong or missing (4, 5). The rest is hygiene.
- "U53" 4 (the unit harnesses' nil rule) and "BRAND" 1 (above).
- The older sections (CS0, HYG2 exposed, BM-A, CA, CS2, CCR2, FUP1, PF-B, PWR, INT-1F-A, OF-A,
  FUP2) mark their items closed or open in place. CCRDOC did not re-verify them.

## Goal

A feature-complete **Rust** port of Path of Building v2.67.2 (`.PathOfBuilding/`, commit `16de4b82d`) that looks like PoB.

- **Calc:** a hand port of ModParser and the Calc* modules, with no Lua at runtime. PoB under LuaJIT is used only as the test oracle (`crates/pob-oracle`).
- **UI:** a port of PoB's own UI on a SimpleGraphic-equivalent renderer (`pob-sg`).
- **Accuracy:** a few % of DPS error is acceptable, but the plan aims for exact.

## Binding documents (read in this order)

1. `docs/port/AMENDMENTS.md`: reconciles the two docs with what was built. It wins on conflict.
2. `docs/port/calc-architecture.md`: unit table in §11.
3. `docs/port/ui-architecture.md`: unit table in §12.
4. `docs/port/ccr/`: contract change requests. Each file's first `Status:` line gives the final decision (AMENDMENTS A7, A8, A10-A14).
5. `docs/port/research/`: the original maps of the Lua and Rust code bases.
6. `docs/port/FOLLOWUPS.md`: open items found after a unit merged. `docs/port/STYLE.md`, `contracts.md` and `testing.md` hold the porting rules, the frozen contracts and the test harness.

## History

The sections below are the dated history of the port (oldest first). Their "in flight" and "not
started" lists are superseded by the current status above.

### Paused 2026-10-10

Paused 2026-10-10 10:05 CEST, on the user's request. Branch: **`rust-port`** (integration branch).

#### Progress at the 2026-10-10 pause

##### Merged into `rust-port`

| Group | Units |
|---|---|
| Foundations | pob-oracle split + oracle harness, pob-dump (data dump), pob-sg (renderer) |
| Calc Wave 0 | P1 (dump + parse oracle + codegen), P2 (pob-lua), P3 (probe oracle), P4 (port scripts), T1 (lua2rs transpiler) |
| Calc Wave 1 | U0 (calc skeleton + frozen contracts) |
| UI Wave 0 | UI-U0 (UI skeleton: pob-sg / pob-sg-wgpu / pob-host / pob-ui-core / pob-app / pob-ui-oracle / app/pob-port-*) |
| Contracts | CCR1 (applied CCRs 0001–0013); CCR2 (decided CCRs 0014–0094, AMENDMENTS A8) |
| Calc Wave 2 | MS-A (ModStore/ModDB/ModList), CT (calc tools), MP0 (ModParser core), LUT (timeless/abyss LUTs) |
| UI Wave 1 | U1 (sg core), U2 (fonts), U3 (images/loaders) |

##### Started but not merged (work committed on `port/<id>` branches)

| Stage | Units |
|---|---|
| Fix round done, awaiting re-review then merge | HYG1, IT-B, BM-B, MS-B, ORC-X, U6, U8 |
| Implementation committed, awaiting review | U4, U5, U7, MP1, MP2, PT, IT-A |

Their worktrees are still under `.claude/worktrees/wf_cfba68ed-a85-*`. They may contain uncommitted edits from agents stopped mid-fix. The committed branch heads are the source of truth.

##### Not started

| Plan | Units |
|---|---|
| Calc Wave 3 | MP3–MP7, IT-C, PS-A/B/C, BM-A, CO-A/B/C, AS, SK-A/B, PF-A..G, DF-A..G, OF-A..I |
| Calc Wave 4 | CS0–CS3 (gate G1b: vertical slice + speed vs LuaJIT) |
| Calc Wave 5 | TR-A/B, MI, CA, PWR |
| Calc Wave 6 | INT-1..4 (parity triage, performance, breakdowns, delete pob-engine/pob-data) |
| UI Wave 2 | U9–U14 (generic controls) |
| UI Wave 3 | U15–U22 (calc facade, Main, BuildList, Build shell, NotesTab end-to-end gate) |
| UI Wave 4 | U23–U45 (tree view, all tabs) |
| UI Wave 5 | U46–U54 (CompareTab, trade/network extras, web calc, cut-over) |

Roughly 22 of about 125 units are merged. Little of the calc or UI logic itself is ported yet: the waves so far built the foundations, oracle and contracts. The calc runs end to end on stubs that return default values. Nothing renders PoB's screens yet; only the renderer demos do.

### How to resume (the 2026-10-10 workflow; history)

The units are driven by a workflow script, the "unit DAG". For each unit it:

1. implements the unit in a worktree on `port/<id>`;
2. runs an adversarial review;
3. runs one fix round;
4. merges into `rust-port`, one unit at a time.

At most 3 heavy agents run at once. Script (session-local copy):
`~/.claude/projects/-home-ferm-Work-projects-PathOfBuildingMK2/73b40f39-f83c-4e1d-8600-9664c7856732/workflows/scripts/port-unit-dag-wf_b29ad431-80a.js`

- **Same session:** resume the stopped run with `resumeFromRunId: wf_cfba68ed-a85` and the same args. Finished agents replay from cache.
- **New session:** ask Claude to "resume the Rust port from docs/port/STATUS.md". It should:
  1. rerun the DAG for the 14 in-flight units, giving the agent the existing `port/<id>` branch to continue from (start with review, not re-implementation);
  2. schedule calc Wave 3 and UI Waves 2–3;
  3. decide new CCRs between waves.

#### Environment notes

- **sccache** is at `~/.cargo/bin/sccache` (not on PATH).
- **Serialised commands:** use `scripts/port/cargo.sh` (flock-serialised cargo) and `scripts/port/oracle.sh`.
- **Oracle corpus:** `.parity/builds` holds 57 builds; oracle dumps are in `.parity/oracle/`; the probe cache is `.parity/probe-cache/`.
- **Data dump:** `data/dump/`. Regenerate it with `cargo run -p pob-dump --release -- --pob .PathOfBuilding/src --out data/dump`.
- **Host architecture:** this machine is **aarch64**. LuaJIT is dual-number here. Decision: the product follows x64 PoB semantics; the oracle runs with `jit.off()`; ±0 differences are tolerated.
- **Legacy app:** the old egui app (`app/pob-desktop`, PoB Lua at runtime) still builds and runs. It is replaced at cut-over (UI U53, calc INT-4). *Since 2026-11-12: replaced; U53 deleted it and INT-4 deleted `pob-engine`/`pob-data`/`pob-extract` (sections below).*
- **Wine:** not installed. The real-SimpleGraphic pixel checks (ui doc T6) stay pending until it is.

### Update 2026-11-10 03:30 (resumed 2026-11-09 via /goal "complete the full Rust port")

Merged since the resume (unique units): AS BM-A BM-B CO-A CO-B CO-C CS0 CS1 CS3 DF-A DF-B DF-C DF-D DF-E DF-F DF-G HYG1 HYG2 IT-A IT-B IT-C MI MP1 MP2 MP3 MP4 MP5 MP6 MP7 MPFIX MS-B OF-A OF-B OF-C OF-D OF-E OF-F OF-G OF-H OF-I ORC-X PF-A PF-C PF-D PF-E PF-F PF-G PS-A PS-B PS-C PT SK-A SK-B TR-A TR-B U10 U11 U12 U13 U14 U4 U5 U6 U7 U8 U9 

Still in flight at this point (workflows of session 8879d008, driver `port-unit-dag-v5`):
CS2, U15-U22 (UI Wave 3 → NotesTab gate), TR-A, OF-B/DF-B leftovers if any, HYG3, MPFIX (parser L1 99.775% → 99.95%), BM-B integration fix (CO-A `enemyRadius` control).
Not started: calc CA, PWR, INT-1..4; UI Wave 4 (U23-U45) and Wave 5 (U46-U54).

Process changes made in this session (see memory + commits):
- `scripts/port/cargo.sh`: 2 compile slots, 3 run slots (tests no longer hold a compile slot), each run in a 5 GB systemd scope, rust-lld, /tmp worktrees build in ~/.cache/pob-port-targets, quick corpus (+POB_QUICK_CORPUS=1) for tests run in the main checkout.
- `scripts/port/oomguard.sh` (transient unit pob-port-oomguard), `scripts/port/full_suite.sh` (transient timer pob-port-fullsuite, every 3 h; results .parity/fullsuite/latest/summary.txt), `scripts/port/quick_parity.sh`, `scripts/port/with_merge_lock.sh` (orchestrator commits).
- Cross-workflow merge lock /tmp/pob-port-merge.lock. Open review issues land in docs/port/FOLLOWUPS.md.
- Resuming a killed run: do NOT use Workflow resumeFromRunId for these concurrent DAGs (it re-runs finished units). Derive per-unit state from the run journals, commit WIP in killed worktrees, and relaunch v5 with explicit `start` (impl | resume | review | fix | merge) per unit.

### Paused 2026-11-10 ~14:50 (user: weekly usage nearly spent)

90 units merged since the 2026-11-09 resume. Calc: every plan unit merged except INT-1..4; parity after CS2
(full 57-build corpus) L6 headline 728/741, keys 44,908/45,954; INT-1A (docs/port/parity/INT-1-triage.md)
showed fixes A+B+C give 741/741 and 45,954/45,954 with 0 Rust errors. INT-1-C is merged. UI: U1-U22, U28,
U29, U32, U39, U43 merged.

Resume with `port-unit-dag-v5` (session 8879d008: workflows/scripts/port-unit-dag-v5-wf_51a98136-ac5.js)
and explicit starts. Do NOT use resumeFromRunId. All workflows were stopped; in-flight work is committed on
the port/* branches (commits titled '<id>: WIP (paused 2026-11-10; unverified)'):

| Unit | Start stage |
|---|---|
| RETYPE (A8 E1-E5, orchestrator-approved) | review (WIP on branch; was in review). A parallel fix attempt that diverges from port/RETYPE is saved on `port/RETYPE-fixwip`: merge or cherry-pick what is useful in the fix round |
| INT-1-A (triage fix A, AS) | review |
| INT-1-B (triage fix B + exact L6 gates) | done on port/INT-1-B (2026-11-11): INT-1 all57 gate met, see below |
| U23 | review |
| U41 | review (fix round 1 committed) |
| U30, U40, U44, U45 | resume (WIP) |
| U24, U25, U26, U27, U31, U33-U38, U42 | impl (deps per ui doc §12.6) |
| INT-1 fresh-100 poe.ninja corpus, INT-2, INT-3, INT-4, UI Wave 5 (U46-U50, U53; U51/U52/U54 optional) | not started |

Machine services (transient systemd user units, recreate after a reboot): pob-port-oomguard (running),
pob-port-push (hourly push of rust-port to GitHub, running), pob-port-fullsuite (stopped for the pause).

Superseded/orphaned agent commits found at the pause are kept as `refs/archive/orphan-<sha>` (not branches); all their units are merged.

### INT-1 all57 gate: done (INT-1-B, 2026-11-11)

INT-1-A (CCR-0113), INT-1-C and INT-1-B together meet the all57 half of INT-1's gate (calc doc
§11.1 row INT-1; docs/port/parity/INT-1-triage.md §8). Full corpus on rust-port 4870179 +
INT-1-B: L6 headline 741/741, all keys 45,954/45,954, CALCS lists 171/171, used-tracking
513/513, misc calculator 1,248/1,248, 0 Rust errors; D13 62/62 fixed points (MAIN, and now
CALCS too), 0 errors. `CA.rs` gates all of these as exact (no floors left). Still open for
INT-1: L6 on 100 fresh poe.ninja builds (a new corpus and a new triage round).

### INT-1 fresh-100: measured and triaged (INT-1-FRESH, 2026-11-12)

The fresh corpus has 100 poe.ninja builds (105 fetched, 5 dropped because their character API
returned 404, 0 dropped for a PoB error or for game data; docs/port/parity/fresh-corpus.md). On
rust-port 645e2e1 with `CA_CORPUS_DIR=.parity/fresh`, the results are:

- L6: headline 1,300/1,300, all keys 85,603/85,606 (every PoB key exact; 3 keys are Rust-only),
  CALCS lists 300/300, used-tracking 900/900, misc 2,200/2,200, 0 Rust errors.
- D13: 99/100 fixed points.

docs/port/parity/INT-1-fresh-triage.md finds two buckets. Both are XS:

- **INT-1F-A** (OF-C, CCR-0019 §8): costs_passes.rs ignores `levelCostCleared`, so a triggered
  item curse keeps its mana cost and adds a `ManaCostWarningList` entry. The candidate fix
  measured 85,603/85,603.
- **INT-1F-B** (CA): fresh_017 is not a fixed point on a warm cache in PoB itself, so it needs a
  `D13_POB_WARM_CACHE` entry.

### INT-1: done (INT-1F-A, 2026-11-12)

INT-1F-A (CCR-0133, amending CCR-0019 §8) fixes bucket A of INT-1-fresh-triage.md:
`calc_offence/costs_passes.rs` (CalcOffence.lua:1720) now treats the cost of a level that
SkillsTab.lua:1190 emptied (`skills_tab::levelCostCleared`) as `{}`, and `CA.rs` resets that
per-process state per build (`skills_tab::reset_level_cost_cleared`), as the oracle boots a fresh
PoB per build. Measured on rust-port fa733f3 + INT-1F-A:

- fresh corpus (`CA_CORPUS_DIR=.parity/fresh`) L6: 100 builds, 0 Rust errors, headline
  1,300/1,300, all keys **85,603/85,603** (the 3 Rust-only `ManaCostWarningList` keys are gone,
  nothing else changed: `CA_L6_DIFFS` wrote no line), CALCS lists 300/300, used-tracking
  900/900, misc 2,200/2,200.
- all57 (`POB_FULL_SUITE=1`) L6 unchanged: headline 741/741, keys 45,954/45,954, CALCS lists
  171/171, used-tracking 513/513, misc 1,248/1,248, 0 Rust errors; D13 62/62 fixed points.
- fresh D13: 99/100, fresh_017 differs, which is PoB's own warm-cache behaviour. That is
  INT-1F-B (on `port/INT-1F-B`, not on rust-port when INT-1F-A finished): with it merged, both
  halves of INT-1's gate (L6 exact on all57 and on 100 fresh poe.ninja builds, D13 everywhere)
  hold. See FOLLOWUPS "INT-1F-A".

### U53 cut-over (2026-11-12)

On `port/U53` (merged rust-port, fix round 1). CCR-0145 lists every cross-unit edit.

- **E2E with the Rust calc, no stand-ins:** `crates/pob-ui-oracle/scenarios/e2e/*.pobscript`
  (15 scenarios: build list, tree open/hover/allocate/undo at 1280x800 and at 1920x1080@1.5,
  tree search/zoom/drag, a right click on an allocated jewel socket (to the Items tab and its
  socket viewer), the Items tab's jewel-socket hover (the socket viewer), Skills, Items +
  undo/redo, Calcs, Config (a check box ticked: a recalc), Notes + Ctrl+S, Party,
  Import/Export, Compare) under `tests/u53_e2e.rs`: unmodified PoB Lua vs
  `pob_app::PobApp::new` (RustCalc), 27 runs, 1,668,755 Lua draw ops, **0 T3 divergences**, T4
  33 pairs identical, same window titles, no error on either side. Each scenario's
  `#! expect-*` guards (DrawString text, Lua source files of the last frame, snapshots that must
  differ: a click that allocates, an undo, a config tick, an unequip/undo/redo) fail the run
  when a step misses its target.
- **Every unit harness on the Rust calc:** `LuaCalcBridge::new` now delegates to RustCalc
  (`POB_UI_CALC=lua` for the old bridge; `LuaCalcBridge::lua` for u15 and `lua-calc`). All of
  u16-u52 (35 binaries) pass with it, plus `parity` (54 cases, 42 gallery runs). No text masks:
  no open calc issue needed one.
- **Integration fixes found by the e2e run:** Save of nil `mainActiveSkill(Calcs)` and nil gem
  `enableGlobal1/2` of the calc's own groups (CCR-0145 §3-§4); the stand-ins left in the
  shipping hook defaults (§5): SkillsTab→ItemsTab socket line/tooltip/undo, ItemsTab
  undo/redo, LIST subPath, the breakdown node viewer, the character-import seams, and (fix
  round 1, found by grepping every `not_ported` call) the ItemsTab socket viewer (its Draw
  and AddNodeName), the Items tab's "Manage..." spec button, the tree's right click on a jewel
  socket, and the timeless-jewel list's SetSelItemId/PopulateSlots/AddUndoState. Only U51's
  trade hooks (on hold) still raise. The first spec undo state is the one `PassiveSpec:Load`
  takes (CCR-0123 §2, §8 here): an undo right after loading a cluster-jewel build now restores
  what PoB restores.
- **Perf pass met:** `tests/u53_perf.rs` (release, asserted by default in builds without debug
  assertions): Tree tab draw at 1920x1080 over 72 frames per build, p50 1.65-2.41 ms, p95
  1.86-3.55 ms on the four fixture builds over two runs on the loaded machine (was p50
  2.6-3.7, p95 2.7-4.5 before; CCR-0145 §9).
- **Web:** the grey node icons were the web `Fs::search` not listing the fetched `src/` files,
  so the tree looked for its sprite sheets outside the version directory (CCR-0145 §10, fixed
  in `pob-host`). Checked in headless Chromium (release wasm): node icons drawn, wheel zoom,
  hover tooltips with the worker's stat differences, a click allocates (1/123, Life 60 → 72).
- **Renames:** `app/pob-port-{desktop,web}` → `app/pob-{desktop,web}`; the egui `crates/pob-ui`
  and the legacy apps are deleted (before the T6 gate, which stays open: orchestrator's call).
  `pob-engine`/`pob-data` stay for calc:INT-4 (deleted since, see below). The web page runs the app by default
  (`?app=testcard` for the test card).
- **Open (FOLLOWUPS "U53"):** T6 (Wine not installed).

### INT-4 retire (2026-11-12)

On `port/INT-4` (from rust-port af4f731). The last calc row (calc doc §11.1). AMENDMENTS A13 has
the details.

- **Deleted:** `crates/pob-engine`, `crates/pob-data` and `crates/pob-extract` (the legacy
  extractor and its `pob_diff`, the only user of the other two: `cargo tree --workspace -i
  pob-engine` / `-i pob-data` listed only `pob-extract`, and a grep found no `pob_engine` /
  `pob_data` use anywhere else), `scripts/parity/run.sh` (ran `pob_diff`), the workspace keys
  only they or the legacy egui app used (`thiserror`, `base64`, `quick-xml`, `eframe`, `egui`,
  `rfd`, `wgpu = "22"`), and CI's engine build/test/clippy and manual `pob_diff` jobs (the calc
  job now runs the `mlua` gate, a wasm32 check of the calc crates and the pob-lua / pob-gamedata
  / pob-modparser tests).
- **Kept, dev-only:** `pob-luahost` is `pob-oracle` (A1). The oracle tests and pob-ui-oracle
  (its LuaJIT boot: `jit::PRELUDE`, `install_natives`; its `LuaCalcBridge`: `jit`, `headless`,
  `inject`) need it, so it stays as a dev-only crate; no shipping package depends on it.
- **Gates:** `cargo tree -p pob-calc -e normal | grep -c mlua` = **0**; the same count is 0 for
  every runtime crate (pob-lua, pob-gamedata, pob-modparser, the six calc crates, pob-sg,
  pob-sg-wgpu, pob-host, pob-ui-core) and for the products **pob-app 0, pob-desktop 0, pob-web 0**
  (pob-app and pob-web also 0 for `--target wasm32-unknown-unknown`). The one path to `mlua` is
  pob-desktop's non-default feature `lua-calc` (3 lines). wasm32 check of all 16 runtime
  packages: green (7 pre-existing wasm32-only dead-code warnings in `power.rs`).
- **Tests:** `cargo.sh check --workspace --tests` green, 0 warnings; quick corpus
  (`.parity-quick`, 14 builds) pob-oracle `CA`: 5/5 passed (L6 headline 182/182, all keys
  11,990/11,990, CALCS lists 42/42, used-tracking 126/126, misc 308/308, 0 Rust errors; D13 19/19
  fixed points); `pob-ui-oracle --test u53_e2e`: 1 passed (27 runs, 1,668,755 Lua draw ops,
  0 T3 divergences, T4 33 pairs identical).
