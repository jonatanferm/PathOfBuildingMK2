# Path of Building MK2

A Rust port of [Path of Building Community](https://github.com/PathOfBuildingCommunity/PathOfBuilding)
v2.67.2: PoB's calculations ported by hand (`pob-calc`, no Lua at runtime) and PoB's own UI ported
onto a SimpleGraphic-equivalent renderer (`pob-sg`), for the desktop and the web. PoB itself, run
under LuaJIT, is used only as the test oracle.

## Status

See [docs/port/STATUS.md](docs/port/STATUS.md) for the port's progress and parity numbers. The
binding design documents are [docs/port/AMENDMENTS.md](docs/port/AMENDMENTS.md) (read first),
[docs/port/calc-architecture.md](docs/port/calc-architecture.md) and
[docs/port/ui-architecture.md](docs/port/ui-architecture.md); testing is described in
[docs/port/testing.md](docs/port/testing.md). [STATUS.md](STATUS.md) and
[docs/parity-plan.md](docs/parity-plan.md) describe the earlier Lua-at-runtime app (history).

## Workspace layout

| Crate | Role |
|---|---|
| `crates/pob-lua` | Lua 5.1 / LuaJIT semantics the port relies on: numbers and formatting, patterns, `table.sort`, LuaJIT's integer-key order, `Name`. |
| `crates/pob-gamedata` | PoB's post-processed `data` table (loaded from `data/dump/data.bin`), `Mod`/`Tag`/`Value`, ModTools, timeless/abyss jewel LUTs. |
| `crates/pob-modparser` | The port of `Modules/ModParser.lua`. |
| `crates/pob-calc` (+ `pob-calc-{core,setup,config,defence,offence,perform}`) | The port of PoB's calculations (`Modules/Calc*`, the build model, the power report). `pob-calc` is the facade. |
| `crates/pob-sg`, `crates/pob-sg-wgpu`, `crates/pob-host` | SimpleGraphic equivalent (draw API, fonts, images), its wgpu backend, and the desktop/web host. |
| `crates/pob-ui-core`, `crates/pob-app` | The port of PoB's own UI: the control framework and every mode and tab, on `pob-calc`. |
| `app/pob-desktop` | The desktop app (`pob_host::run_desktop` with `pob_app::PobApp`). |
| `app/pob-web` | The browser app (wasm32 + Trunk). |
| `crates/pob-dump` | Build-time tool: boots PoB under LuaJIT and dumps its `data` table to `data/dump/` (`data.bin`). |
| `crates/pob-oracle` | Dev only: PoB run headless under LuaJIT, the calc's test oracle (probe oracle, `pob_oracle`, `pob_calc_diff`). |
| `crates/pob-ui-oracle` | Dev only: PoB's Lua UI under LuaJIT drawing through `pob-sg`, the UI parity harness and `LuaCalcBridge`. |
| `crates/pob-testpaths` | Dev only: locates the gitignored test inputs. |

No shipping crate depends on `mlua`: `cargo tree -p <crate> -e normal | grep -c mlua` is 0 for
`pob-calc`, `pob-app`, `pob-desktop` and `pob-web`. The non-default desktop feature `lua-calc`
(PoB's Lua calc through `LuaCalcBridge`, for development) is the only exception.

## Building

```bash
# One-time: the upstream PoB checkout (gitignored) and the data dump.
git clone https://github.com/PathOfBuildingCommunity/PathOfBuilding .PathOfBuilding
cargo run -p pob-dump --release -- --pob .PathOfBuilding/src --out data/dump
cargo run -p pob-dump --release -- --compile --out data/dump   # writes data/dump/data.bin

cargo run -p pob-desktop --release
```

`data/dump/` is gitignored. Tests that need it, the PoB checkout or the parity corpus fail with a
message naming the missing input, or skip under `POB_ALLOW_SKIP=1`. Inside the port's own
workflow, run cargo through `scripts/port/cargo.sh` (machine-wide build slots and memory caps;
docs/port/testing.md).

### Continuous integration

[`.github/workflows/ci.yml`](.github/workflows/ci.yml) runs on every PR and push to `main`:

- the calc job: the "no `mlua` at runtime" gate, a wasm32 check of the calc crates and the
  `pob-lua` / `pob-gamedata` / `pob-modparser` tests;
- the UI job: builds `pob-desktop`, checks `pob-web` for wasm32, runs clippy on the UI crates and
  the `pob-sg` / `pob-ui-core` tests;
- `cargo fmt --check`.

## Reference repo

The upstream Lua codebase lives in `.PathOfBuilding/` (a hidden subdir, gitignored) inside this
repo; `pob-dump` and the oracles read it from there.
