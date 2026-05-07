# Project status

## What works today

A buildable, testable, runnable Path of Building MK2 desktop app.

- **`cargo build --workspace`** — all 5 crates compile clean.
- **`cargo run -p pob-extract --release`** — extracts 28 modern PoE passive
  trees, 1062 item bases, and 810 skill gems from the sibling
  `../PathOfBuilding/` checkout into `data/`.
- **`cargo test --workspace`** — 44 tests pass (engine: 31 unit + 1
  parser-coverage + 2 pathfind, data: 4 unit + 5 round-trip, ui: 1 layout).
- **`cargo run -p pob-desktop --release`** — opens the app. Drag to pan,
  scroll to zoom, type in the search box to highlight matching nodes,
  hover an unallocated node to preview the shortest path from your
  allocated cluster, click to allocate. Stats panel updates live.

End-to-end demo: select Marauder, click a `+10 Strength` notable on the
tree → Strength climbs 32 → 42 and Life climbs 66 → 71.

## Module map

| Crate | Status |
|---|---|
| `pob-data` | Types + JSON loaders for tree, bases, gems. Done for Phase 1. |
| `pob-engine` | Mod, ModDB, basic ModParser (37% coverage on the 3.25 tree), CalcSetup, basic-stats CalcPerform. |
| `pob-extract` | mlua-driven extractor for tree / bases / gems. Skills are deferred (ADR 0002). |
| `pob-ui` | Passive-tree screen with pan/zoom/search/path-preview + live stats panel. |
| `pob-desktop` | Thin eframe wrapper over `pob-ui`. |

## What's next, in roughly the order I'd tackle it

1. **Items**: add an `Item` type + `ItemSet`, parse copy-paste item text,
   plumb item mods into the env. Unblocks most defensive computation.
2. **Skill data extraction**: solve the deferred problem from ADR 0002 —
   teach the extractor to record `mod()` / `flag()` / `skill()` calls
   instead of running them. Then we can compute DPS for one skill.
3. **Validation harness against live PoB**: drive PoB headless via Lua,
   import a build XML on both sides, diff `env.player.output`. CI-friendly
   target: exact match on stats both engines compute.
4. **ModParser expansion to ~70%**: the `Attacks have N% chance to`,
   `while X`, `per Y` forms are the next batch.
5. **Other UI tabs**: skills tab, items tab, calcs breakdown, config tab,
   import/export from POB share codes. Engine is already separated, so the
   tabs are mostly ergonomic UI work.

## Known divergences / shortcuts

- **Range mods collapse to min**: `+(20-30) to Strength` is currently a
  flat `+20`. Real PoB averages or rolls; we'll surface both bounds in
  Phase 4 alongside item rolls.
- **Per-X scalings drop the multiplier**: `1% increased Damage per Power
  Charge` parses as `1% increased Damage` with no scaling — the value
  shows up at 1× instead of `(power_charge_count)×`. Eval supports the
  `Multiplier` tag, so a parser pass that emits the tag will fix this.
- **Conditional clauses ignored**: `while at full life`, `if you've killed
  recently`, etc. The engine has the `Condition` tag wired up; the parser
  just doesn't emit it yet.
- **Skill data not yet extracted** — see ADR 0002.

`docs/decisions/` has ADRs for the workspace layout (0001) and the
deferred skill extraction (0002). Add a new one whenever you make a call
that's not obvious from the code.
