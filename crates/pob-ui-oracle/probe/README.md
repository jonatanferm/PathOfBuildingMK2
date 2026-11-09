# U2 font probe: real SimpleGraphic metrics under Wine (ui doc §11.1 T2/T6)

The Rust font engine (`crates/pob-sg/src/font/`) is a port of SimpleGraphic's `r_font.cpp` at
`3b1a346`, the commit the shipped `SimpleGraphic.dll` identifies itself as
(`SimpleGraphic v2.5-3b1a346`). This directory holds the tools that check it.

| File | Purpose |
|---|---|
| `rfont_ref.py` | Independent Python transcription of `r_font.cpp` (+ `IndexUTF8ToUTF32`, colour escapes) with emulated f32 arithmetic. Generates the T2 goldens and the case table. |
| `cases.lua` | Generated case table (200 strings, heights 12/16/22/30, fonts VAR/VAR BOLD/FIXED, 10 cursor points). Shared by the goldens and the probe. |
| `Launch.lua` | The probe: a SimpleGraphic script that measures every case with the real DLL and draws a screenshot scene. |
| `run.sh` | Runs the probe under Wine and copies the output to `expected/`. |
| `metrics.schema.json` | Schema of `out/metrics.json` / `expected/metrics.json`. |
| `expected/` | Probe output (absent until Wine has run): `metrics.json`, `scene.json`, `scene.png`. |

## T2 (gates U2): goldens from the reference transcription

`crates/pob-sg/tests/font_goldens.json` is written by `rfont_ref.py`, not by the Rust code, so
`font_goldens.rs` is a differential test of two independent transcriptions (2400 cases: width,
10 cursor indices and a digest of the full BIND/COLOR/QUAD stream of two draws each). To change
the case set, edit `HAND_STRINGS`/`GOLDEN_*` in `rfont_ref.py`, then:

```sh
F=/home/ferm/Work/projects/PathOfBuildingMK2/.PathOfBuilding/runtime/SimpleGraphic/Fonts
python3 crates/pob-ui-oracle/probe/rfont_ref.py --fonts "$F" goldens crates/pob-sg/tests/font_goldens.json
python3 crates/pob-ui-oracle/probe/rfont_ref.py --fonts "$F" cases-lua crates/pob-ui-oracle/probe/cases.lua
scripts/port/cargo.sh test -p pob-sg --test font_goldens
```

`rfont_ref.py --fonts "$F" width|cursor|draw ...` evaluates single cases (used to derive the hand
cases in `crates/pob-sg/tests/font_{width,cursor,draw}.rs`).

## T6 (pending until Wine is installed): the real DLL

Wine is not installed on the build machine (AMENDMENTS A5); agents cannot install it. Once the
user has run `pacman -S wine`:

```sh
crates/pob-ui-oracle/probe/run.sh            # uses <repo>/.PathOfBuilding/runtime
scripts/port/cargo.sh test -p pob-sg --test font_probe -- --nocapture
```

`run.sh` copies `.PathOfBuilding/runtime/` (the launcher exe, `SimpleGraphic.dll`, `lua51.dll`,
`SimpleGraphic/Fonts`) to a temp directory, puts `Launch.lua` and `cases.lua` next to the exe (the
launcher picks up `Launch.lua` there; its first line `#@ SimpleGraphic` names the DLL), runs it,
and copies `out/metrics.json`, `out/scene.json` and the newest `Screenshots/*.png` to `expected/`.
The probe exits by itself after 8 frames.

What the probe does:

1. `OnInit`: `RenderInit("DPI_AWARE")`, `SetDPIScaleOverridePercent(100)`, then for every font,
   height and string of `cases.lua`: `DrawStringWidth(h, font, s)` and
   `DrawStringCursorIndex(h, font, s, x, ky*h + ay)` for each point → `out/metrics.json`.
2. Every frame: draws the scene listed in `Launch.lua` (all 5 aligns, escapes including the
   escape-colour leak into a following `DrawImage`, tabs, newlines, tofu, scaled heights, a
   clipped and an off-screen line); the calls are also written to `out/scene.json`.
3. Frame 5: `TakeScreenshot()` (format forced to png). Frame 8: `Exit()`.

`crates/pob-sg/tests/font_probe.rs` compares `expected/metrics.json` with
`FontFace::string_width` / `cursor_index` (+1) exactly; while the file is absent it reports
PENDING and passes (and still self-tests the comparator on synthetic data). The screenshot
comparison (pixels ±2/255 on ≥ 99.5 %) needs the CPU rasteriser (U6) and is U53's; `scene.json`
holds everything needed to replay the scene through `pob-sg`.

Caveats to check on the first real run: the window size the launcher picks (recorded as
`screenSize`), and that `screenScale` is 1 (the comparison refuses other scales).
