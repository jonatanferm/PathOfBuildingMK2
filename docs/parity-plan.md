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
2. **ModParser** (`Modules/ModParser.lua`) — oracle: `Data/ModCache.lua`
   (~20k lines with PoB's own parse result). Target ≥ 99% identical.
   ModCache is also used at runtime as PoB does (fast path).
3. **Data**: everything the calc reads from `Modules/Data.lua`, `Data/*.lua`
   (skills incl. statMap, SkillStatMap, gems, bases, uniques, misc tables).
4. **CalcSetup → CalcActiveSkill → CalcPerform → CalcDefence →
   CalcOffence → CalcTriggers / CalcMirages**, validated per stage.
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
