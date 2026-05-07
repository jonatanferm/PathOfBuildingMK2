# ADR 0001 — Workspace layout

**Status:** accepted
**Date:** 2026-05-07

## Context

PoB is one big bag of Lua. The Rust port needs to separate concerns from day one so the
engine stays pure (Wasm-clean) and the UI stays thin.

## Decision

Cargo workspace with five members:

- `pob-engine` — pure calc, no I/O, no UI, no `std::fs`. The Wasm target.
- `pob-data` — types + loaders. Engine consumes types from here. UI also consumes them.
- `pob-extract` — build-time tool that converts PoB's `src/Data/*.lua` and
  `src/TreeData/*/tree.lua` into Rust-loadable formats under `data/`.
- `pob-ui` — egui code. May depend on engine + data, never the other way around.
- `pob-desktop` — thin binary wrapping `pob-ui` in eframe.

Reference clone lives at `../PathOfBuilding/` (sibling dir, gitignored within this repo).

## Consequences

- `pob-engine` cannot use `tokio`, `std::fs`, `eframe`, `egui`, `wgpu`, or `mlua`. CI lints
  enforce this once we add the lints.
- `pob-extract` is the *only* place that depends on `mlua`. It runs at build time and
  emits artefacts under `data/`. Runtime never touches Lua.
- `pob-data` is the contract between engine and UI, so any breaking type change is
  inherently visible to both sides.

## Alternatives considered

- **Single crate with feature flags.** Rejected: too easy for engine code to silently
  pull a UI dep transitively.
- **Putting extraction inside `pob-data` build.rs.** Rejected: `pob-data` would gain
  `mlua` as a build-dep transitively, making downstream builds slower and breaking Wasm.
  Keeping extraction as a separate binary tool is cleaner and the output is committed
  (or generated once and cached).
