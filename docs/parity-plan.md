# Parity plan — faithful port of PoB's calc pipeline

Status as of 2026-09-29. Owner: the parity branch (`parity/real-builds`).

## Why

Measured on 56 real builds (52 current-league poe.ninja characters + the 5
upstream PoB spec builds), the existing `pob-engine` calc matches PoB on only
~56% of shared output keys, and on almost none of the headline stats for
real endgame characters. The existing `perform.rs` (~5.4k lines) is a
heuristic re-implementation, not a port: PoB's calc core is ~19k lines
(CalcSetup / ActiveSkill / Perform / Defence / Offence / Triggers / Mirages)
plus a 7k-line ModParser. Patching the heuristic engine does not converge.

## Approach

Port PoB module-by-module into `crates/pob-engine/src/calc/`, keeping PoB's
structure, names and output keys so each piece can be diffed against the Lua
it came from. The old engine stays in place until the new one beats it on the
parity corpus, then the UI switches over and the old code is deleted.

1. **Mod model + ModStore semantics** (`Classes/ModStore.lua`, `ModDB.lua`,
   `ModList.lua`, `Modules/ModTools.lua`): full tag set and evaluation.
2. **ModParser** — DONE (2026-09-29): PoB's `Modules/ModParser.lua` runs
   *unchanged* in `pob_calc::lua`, a small pure-Rust Lua 5.1 interpreter
   (Wasm-clean; exact Lua pattern semantics via `pob_calc::lua_pattern`).
   The only patch swaps the hot `scan` helper for a native equivalent.
   Oracle `tests/modcache_oracle.rs`: 23,204 / 23,207 `Data/ModCache.lua`
   lines identical (the 3 others are stale cache entries). ~0.7 ms/line,
   cached per line. Rationale: the parser is a ~4k-entry rule table with
   Lua closures that PoB edits every league — running it verbatim keeps
   parity for free on upstream pulls, where a hand port would drift.
   Env data (`data/lua/modparser_env.lua`) comes from
   `pob_diff --dump-modparser-env`.
3. **Data**: everything the calc reads from `Modules/Data.lua`, `Data/*.lua`
   (skills incl. statMap, SkillStatMap, gems, bases, uniques, misc tables).
4. **CalcSetup → CalcActiveSkill → CalcPerform → CalcDefence →
   CalcOffence → CalcTriggers / CalcMirages**, hand-ported to Rust
   (performance matters: the UI recomputes on every click and the power
   heatmap runs one calc per tree node), validated per stage.
5. **Switch UI + features** (breakdowns come from the calc itself, as in PoB).
6. **Feature gaps** vs PoB's UI (unique DB, crafting, trade, etc.).

## Tooling

- `scripts/parity/fetch_ninja.py [league] [type] [n]` — pull current real
  builds into `.parity/builds/` (gitignored).
- `scripts/parity/run.sh` — run `pob_diff` over the corpus in parallel,
  write `.parity/results/*.json`, print the scoreboard (`report.py`).
- `pob_diff --build X.xml --explain Life,Str` — PoB `modDB:Tabulate` vs
  ours, per stat.

## Scoreboard history

| Date | Engine | Builds | Shared keys matching (≤1%) |
|---|---|---|---|
| 2026-09-29 | old `perform.rs` | 56 | 56.2% |
