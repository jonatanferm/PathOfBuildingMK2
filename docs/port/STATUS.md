# Rust port: status and how to resume

Paused 2026-10-10 10:05 CEST, on the user's request. Branch: **`rust-port`** (integration branch, local only, never pushed).

## Goal

A feature-complete **Rust** port of Path of Building v2.67.2 (`.PathOfBuilding/`, commit `16de4b82d`) that looks like PoB.

- **Calc:** a hand port of ModParser and the Calc* modules, with no Lua at runtime. PoB under LuaJIT is used only as the test oracle (`crates/pob-oracle`).
- **UI:** a port of PoB's own UI on a SimpleGraphic-equivalent renderer (`pob-sg`).
- **Accuracy:** a few % of DPS error is acceptable, but the plan aims for exact.

## Binding documents (read in this order)

1. `docs/port/AMENDMENTS.md`: reconciles the two docs with what was built. It wins on conflict.
2. `docs/port/calc-architecture.md`: unit table in §11.
3. `docs/port/ui-architecture.md`: unit table in §12.
4. `docs/port/ccr/`: contract change requests (`Status: applied …` marks the applied ones).
5. `docs/port/research/`: the original maps of the Lua and Rust code bases.

## Progress

### Merged into `rust-port`

| Group | Units |
|---|---|
| Foundations | pob-oracle split + oracle harness, pob-dump (data dump), pob-sg (renderer) |
| Calc Wave 0 | P1 (dump + parse oracle + codegen), P2 (pob-lua), P3 (probe oracle), P4 (port scripts), T1 (lua2rs transpiler) |
| Calc Wave 1 | U0 (calc skeleton + frozen contracts) |
| UI Wave 0 | UI-U0 (UI skeleton: pob-sg / pob-sg-wgpu / pob-host / pob-ui-core / pob-app / pob-ui-oracle / app/pob-port-*) |
| Contracts | CCR1 (applied CCRs 0001–0013) |
| Calc Wave 2 | MS-A (ModStore/ModDB/ModList), CT (calc tools), MP0 (ModParser core), LUT (timeless/abyss LUTs) |
| UI Wave 1 | U1 (sg core), U2 (fonts), U3 (images/loaders) |

### Started but not merged (work committed on `port/<id>` branches)

| Stage | Units |
|---|---|
| Fix round done, awaiting re-review then merge | HYG1, IT-B, BM-B, MS-B, ORC-X, U6, U8 |
| Implementation committed, awaiting review | U4, U5, U7, MP1, MP2, PT, IT-A |

Their worktrees are still under `.claude/worktrees/wf_cfba68ed-a85-*`. They may contain uncommitted edits from agents stopped mid-fix. The committed branch heads are the source of truth.

### Not started

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

## How to resume

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

### Environment notes

- **sccache** is at `~/.cargo/bin/sccache` (not on PATH).
- **Serialised commands:** use `scripts/port/cargo.sh` (flock-serialised cargo) and `scripts/port/oracle.sh`.
- **Oracle corpus:** `.parity/builds` holds 57 builds; oracle dumps are in `.parity/oracle/`; the probe cache is `.parity/probe-cache/`.
- **Data dump:** `data/dump/`. Regenerate it with `cargo run -p pob-dump --release -- --pob .PathOfBuilding/src --out data/dump`.
- **Host architecture:** this machine is **aarch64**. LuaJIT is dual-number here. Decision: the product follows x64 PoB semantics; the oracle runs with `jit.off()`; ±0 differences are tolerated.
- **Legacy app:** the old egui app (`app/pob-desktop`, PoB Lua at runtime) still builds and runs. It is replaced at cut-over (UI U53, calc INT-4).
- **Wine:** not installed. The real-SimpleGraphic pixel checks (ui doc T6) stay pending until it is.
