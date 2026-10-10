# `fixtures/user1` (ui doc §11.3; owner U8)

The user dir of the app scenarios. `GetUserPath()` returns a fresh run dir and PoB reads
`<userPath>/Path of Building/` (Main.lua:94-98), so this directory is copied to
`<run dir>/Path of Building/` before each side of a run (`runner::UserDir`). Runs never modify
these files.

* `Settings.xml`: starts in the build list (`<Mode mode="LIST">` with `GetArgs()`'s two
  arguments: no selected build, root sub-path), build sort by name, the `main:Init` defaults
  otherwise, `showPublicBuilds="false"`.
* `Builds/`: 6 builds of the parity corpus (`.parity/builds`, see `scripts/parity`) whose oracle
  `TotalDPS` is > 0 (`.parity/oracle/<stem>/outputs.json`):

| file | TotalDPS (oracle) |
|---|---|
| allflame_exp_001.xml | 2 145 267.9 |
| allflame_exp_007.xml | 1 912 377.8 |
| allflame_exp_030.xml | 4 932 858.2 |
| allflame_exp_048.xml | 19 564 115.2 |
| upstream_Dual_Wield_Cospris_CoC.xml | 16 493 623.8 |
| upstream_Generals_Perforate_Zerker.xml | 22 360 547.4 |

Scenarios open one with `load-build "Builds/<file>"` (relative to `Path of Building/`).
