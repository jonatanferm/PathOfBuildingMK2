# Project status

Status as of 2026-09-30 (branch `parity/real-builds`).

## Architecture in one paragraph

MK2 runs **Path of Building's own, unmodified Lua** headlessly and puts a
Rust/egui UI on top. On desktop PoB runs under vendored **LuaJIT**
(`pob_calc::jit`, PoB's own runtime); the web build uses the pure-Rust Lua
interpreter in `pob_calc::lua` (`pob_calc::headless`). Both implement
`headless::Engine` and give identical numbers. A worker thread
(`pob_calc::worker`) owns PoB; the UI sends Lua commands/queries and gets
snapshots back (PoB's build XML, outputs, sidebar, warnings, sets, skill
selectors). PoB's XML is mirrored back into MK2's `Character` model after
every change. Most tabs now render PoB's own UI controls through a generic
bridge (`__mk2_ctl` / `__mk2_set_ctl` in `headless::BOOT_SCRIPT`,
`pob-ui/src/pob_controls.rs`), so behaviour follows PoB exactly.

## Calculation parity

- 57-build corpus (52 poe.ninja 3.29 builds + 5 PoB spec builds):
  **38,966 / 38,966 outputs identical** between the LuaJIT and Rust-Lua
  engines, and identical to the real-LuaJIT oracle (`pob_diff`).
- The old hand-written engine (`pob-engine/perform.rs`) scores ~53% and is
  only used on the web build and as a placeholder before PoB answers.

## Performance (desktop, LuaJIT)

| | LuaJIT | Rust interpreter |
|---|---|---|
| Boot | 1.9 s | 2.7 s |
| Load a build | 0.4 s | 2.2 s |
| Recalculate | 19–41 ms | 95–150 ms |
| Full power report (heatmap) | 4.6 s | 46 s |
| Node / item tooltip with comparisons | ~20 ms | ~45 ms |

`MK2_POB_ENGINE=rust` forces the interpreter on desktop.

## Feature coverage vs PoB

| Area | State |
|---|---|
| Open / save / auto-save | PoB XML verbatim in and out (Save, Save As, auto-save, Builds tab) |
| Import | PoB codes / URLs / XML; pathofexile.com account import runs PoB's own ImportTab code |
| Export | PoB codes from PoB's own XML; "Export Support" (party buffs) toggle |
| Sidebar | PoB's stat list, warnings, point counter, main-skill / part / stage / minion selectors |
| Tree | Clicks use PoB's `AllocNode` / `DeallocNode`; PoB node tooltips with stat comparisons; masteries preserved; PoB's spec sets (new/copy/rename/delete); heatmap from PoB's `PowerBuilder` |
| Items | PoB's slot list (weapon swap, flask activation, abyssal + tree jewel sockets, item pool dropdowns); PoB's item editor (craft, create/edit from text, affixes, influences, catalysts, enchant/anoint/corrupt/implicit popups); unique + rare databases; PoB item tooltips; item sets |
| Skills | PoB's socket groups and gem-slot controls, gem search over PoB's gem list, gem/group tooltips, skill sets |
| Config | All PoB config options with PoB's visibility rules; config sets |
| Calcs | PoB's calc sections, formatting, breakdowns, skill-select controls |
| Party | PoB's Party tab (import members' buffs from build codes, edit lists) |
| Compare | Builds loaded for comparison are calculated by PoB (`CompareEntry`) |
| Timeless jewels | PoB's "Find a Timeless Jewel" search (tree toolbar) |
| Network (desktop) | PoB's `launch:DownloadPage` over Rust HTTP, `OpenURL`, and PoB's OAuth login (native replacement for its LuaSocket redirect server) — Trader dialog, build-site URLs |
| Dialogs | Any PoB popup renders as a window from PoB's own controls |
| Notes | Yes |

Known gaps: PoB's own compare views (MK2's compare tab and per-mod
attribution still use the old engine), drag-and-drop in item lists,
PoB's update checker; the web build has no network features and uses the
slower interpreter. Trade searching end to end is untested (needs a PoE
login).

## Testing

- `cargo test --workspace` (plus `pob-calc` worker / modcache oracle tests).
- `scripts/parity/run.sh` and `pob_headless` (`POB_ENGINE=jit`,
  `HEADLESS_EXEC`, `HEADLESS_JSON_OUT`) for corpus runs.
- `pob-desktop <build file>` opens a build at start-up (handy for scripted
  UI testing).

## Packaging

`cargo run -p pob-extract --release` extracts data and copies PoB's Lua,
runtime and timeless-jewel data into `data/pob/` (~355 MB), which the app
boots from.
