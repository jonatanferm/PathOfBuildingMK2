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

## Update 2026-09-30: run PoB's own Lua instead of hand-porting

Measurements changed the plan. PoB's full `BuildOutput` takes 14–25 ms
under plain PUC Lua, so interpreting PoB's code is fast enough, and it is
the only route to exact parity that survives PoB's league-by-league churn.

- `pob_calc::lua` grew into a complete-enough Lua 5.1/LuaJIT interpreter:
  byte strings, metatables, `goto`, compound assignment (`+=`, which PoB's
  patched LuaJIT supports), `require`/`loadfile`, `io`, LuaJIT `bit`,
  Lua-style tracebacks.
- `pob_calc::headless::Pob` boots PoB exactly like its own
  `HeadlessWrapper.lua` (skipping the UI-only unique/rare item browser DB).
- The oracle now runs on real LuaJIT (`mlua` vendored `luajit`): the old
  Lua 5.4 oracle was itself wrong in places (integer/float semantics).

Result on the 57-build corpus (52 poe.ninja 3.29 builds + 5 PoB test
builds): **38,381 / 38,384 numeric outputs identical to PoB** (99.992%).
The old hand-written engine scores 53% on the same oracle. Load 1–2 s per
build, recalculation 50–230 ms, boot 2.8 s (interpreter still ~6× slower
than PUC Lua — optimisation pending).

Next: switch the MK2 app to the embedded PoB (Lua model is the source of
truth; the Rust UI binds to it), then optimise the interpreter.

## Approach (original, superseded for the calc — kept for history)

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
| 2026-09-29 | old `perform.rs` (vs Lua 5.4 oracle) | 56 | 56.2% |
| 2026-09-30 | old `perform.rs` (vs LuaJIT oracle) | 57 | 53.3% |
| 2026-09-30 | embedded PoB in `pob_calc::lua` | 57 | 99.992% |
